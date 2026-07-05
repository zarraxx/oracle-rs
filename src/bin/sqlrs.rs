use chrono::{Datelike, Timelike};
use oracle_rs::{
    BindDirection, BindParam, Config, Connection, Error, OracleType, QueryResult, Value,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct BindVariable {
    oracle_type: OracleType,
    buffer_size: u32,
    value: Option<Value>,
}

#[derive(Debug)]
struct Options {
    connect: Option<String>,
    script: Option<PathBuf>,
}

struct SqlRs {
    conn: Connection,
    vars: HashMap<String, BindVariable>,
    echo: bool,
    feedback: bool,
    serveroutput: bool,
    continue_on_error: bool,
    session_time_zone_offset_minutes: Option<i32>,
}

impl SqlRs {
    async fn new(conn: Connection) -> Self {
        Self {
            conn,
            vars: HashMap::new(),
            echo: false,
            feedback: true,
            serveroutput: false,
            continue_on_error: false,
            session_time_zone_offset_minutes: None,
        }
    }

    async fn run_file(&mut self, path: &Path) -> Result<(), String> {
        let content = fs::read_to_string(path)
            .map_err(|err| format!("SP2-0310: unable to open file '{}': {err}", path.display()))?;
        self.run_script(&content).await
    }

    async fn run_script(&mut self, content: &str) -> Result<(), String> {
        let mut buffer = String::new();
        let mut block_mode = false;
        let mut echo_line_no = 1usize;

        for raw_line in content.lines() {
            let line = raw_line.trim_end_matches('\r');
            let trimmed = line.trim();

            if buffer.is_empty() && trimmed.is_empty() {
                if self.echo {
                    println!("SQL> ");
                }
                echo_line_no = 1;
                continue;
            }

            if self.echo {
                if buffer.is_empty() {
                    println!("SQL> {line}");
                    echo_line_no = 2;
                } else {
                    println!("{echo_line_no:>3}  {line}");
                    echo_line_no += 1;
                }
            }

            if buffer.is_empty() && self.handle_command(trimmed).await? {
                echo_line_no = 1;
                continue;
            }

            if trimmed == "/" && block_mode {
                let sql = buffer.trim().to_string();
                buffer.clear();
                block_mode = false;
                self.execute_statement(&sql).await?;
                echo_line_no = 1;
                continue;
            }

            if buffer.is_empty() && starts_plsql_or_create_block(trimmed) {
                block_mode = true;
            }

            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(line);

            if !block_mode && ends_sql_statement(trimmed) {
                let sql = strip_trailing_semicolon(buffer.trim()).to_string();
                buffer.clear();
                self.execute_statement(&sql).await?;
                echo_line_no = 1;
            }
        }

        if !buffer.trim().is_empty() {
            let sql = strip_trailing_semicolon(buffer.trim()).to_string();
            self.execute_statement(&sql).await?;
        }

        Ok(())
    }

    async fn handle_command(&mut self, trimmed: &str) -> Result<bool, String> {
        let upper = trimmed.to_ascii_uppercase();

        if upper.starts_with("SET ") {
            self.handle_set(&trimmed[4..]).await?;
            return Ok(true);
        }

        if upper.starts_with("PROMPT") {
            let text = trimmed.get(6..).unwrap_or("").trim_start();
            println!("{text}");
            return Ok(true);
        }

        if upper.starts_with("WHENEVER SQLERROR") {
            self.continue_on_error = upper.contains("CONTINUE");
            return Ok(true);
        }

        if upper.starts_with("VAR ") || upper.starts_with("VARIABLE ") {
            let rest = trimmed
                .split_once(char::is_whitespace)
                .map(|(_, rest)| rest)
                .unwrap_or("");
            self.define_variable(rest)?;
            return Ok(true);
        }

        if upper.starts_with("PRINT ") {
            let name = trimmed[6..].trim();
            self.print_variable(name)?;
            return Ok(true);
        }

        if upper == "COMMIT" || upper == "ROLLBACK" {
            self.execute_statement(upper.as_str()).await?;
            return Ok(true);
        }

        if upper.starts_with("EXEC ") || upper.starts_with("EXECUTE ") {
            let rest = trimmed
                .split_once(char::is_whitespace)
                .map(|(_, rest)| rest)
                .unwrap_or("");
            self.execute_exec(rest).await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn handle_set(&mut self, rest: &str) -> Result<(), String> {
        let mut parts = rest.split_whitespace();
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let value = parts.next().unwrap_or("").to_ascii_uppercase();

        match name.as_str() {
            "ECHO" => self.echo = value == "ON",
            "FEEDBACK" => self.feedback = value != "OFF",
            "SERVEROUTPUT" => {
                self.serveroutput = value == "ON";
                if self.serveroutput {
                    let _ = self
                        .conn
                        .execute("BEGIN DBMS_OUTPUT.ENABLE(NULL); END;", &[])
                        .await;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn define_variable(&mut self, rest: &str) -> Result<(), String> {
        let mut parts = rest.split_whitespace();
        let name = parts
            .next()
            .ok_or_else(|| "SP2-0552: bind variable name expected".to_string())?;
        let type_spec = parts.collect::<Vec<_>>().join(" ");
        let (oracle_type, buffer_size) = parse_sqlplus_type(&type_spec);
        self.vars.insert(
            normalize_var_name(name),
            BindVariable {
                oracle_type,
                buffer_size,
                value: None,
            },
        );
        Ok(())
    }

    fn print_variable(&self, name: &str) -> Result<(), String> {
        let key = normalize_var_name(name);
        let var = self
            .vars
            .get(&key)
            .ok_or_else(|| format!("SP2-0552: Bind variable \"{}\" not declared.", key))?;
        println!();
        println!("{}", key);
        println!("{}", "-".repeat(key.len().max(1)));
        println!(
            "{}",
            var.value.as_ref().map(format_value).unwrap_or_default()
        );
        println!();
        Ok(())
    }

    async fn execute_exec(&mut self, rest: &str) -> Result<(), String> {
        let trimmed = strip_trailing_semicolon(rest.trim()).trim();
        if trimmed.starts_with(':') && trimmed.contains(":=") {
            for assignment in split_top_level(trimmed, ';') {
                self.apply_assignment(assignment.trim())?;
            }
            if self.feedback {
                println!();
                println!("PL/SQL procedure successfully completed.");
                println!();
            }
            return Ok(());
        }

        self.execute_plsql_call(trimmed).await
    }

    fn apply_assignment(&mut self, assignment: &str) -> Result<(), String> {
        let (left, right) = assignment
            .split_once(":=")
            .ok_or_else(|| format!("SP2-0734: unknown EXEC assignment: {assignment}"))?;
        let name = normalize_var_name(left.trim().trim_start_matches(':'));
        let value = parse_literal(right.trim());
        let var = self.vars.entry(name).or_insert_with(|| BindVariable {
            oracle_type: infer_oracle_type(&value),
            buffer_size: default_buffer_size(&value),
            value: None,
        });
        var.value = Some(value);
        Ok(())
    }

    async fn execute_plsql_call(&mut self, call: &str) -> Result<(), String> {
        let (name, args) = parse_call(call)?;
        let mut params = Vec::new();
        let mut var_positions = Vec::new();
        let mut call_args = Vec::new();

        for arg in args {
            if let Some(var_name) = arg.trim().strip_prefix(':') {
                let key = normalize_var_name(var_name);
                let var = self
                    .vars
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| BindVariable {
                        oracle_type: OracleType::Varchar,
                        buffer_size: 4000,
                        value: None,
                    });
                match var.value {
                    Some(value) => {
                        params.push(BindParam::input_output(value, var.buffer_size.max(1)));
                        var_positions.push(Some(key));
                        call_args.push(format!(":{}", params.len()));
                    }
                    None => {
                        params.push(BindParam::output(var.oracle_type, var.buffer_size.max(1)));
                        var_positions.push(Some(key));
                        call_args.push(format!(":{}", params.len()));
                    }
                }
            } else {
                call_args.push(arg.trim().to_string());
            }
        }

        let sql = format!("BEGIN {name}({}); END;", call_args.join(", "));
        match self.conn.execute_plsql(&sql, &params).await {
            Ok(result) => {
                let mut out_idx = 0usize;
                for (idx, param) in params.iter().enumerate() {
                    if param.direction != BindDirection::Input {
                        if let Some(var_name) = &var_positions[idx] {
                            if let Some(value) = result.out_values.get(out_idx).cloned() {
                                self.vars
                                    .entry(var_name.clone())
                                    .and_modify(|var| var.value = Some(value.clone()))
                                    .or_insert_with(|| BindVariable {
                                        oracle_type: infer_oracle_type(&value),
                                        buffer_size: default_buffer_size(&value),
                                        value: Some(value),
                                    });
                            }
                        }
                        out_idx += 1;
                    }
                }
                self.drain_dbms_output().await?;
                if self.feedback {
                    println!();
                    println!("PL/SQL procedure successfully completed.");
                    println!();
                }
                Ok(())
            }
            Err(err) => self.handle_statement_error(&sql, err),
        }
    }

    async fn execute_statement(&mut self, sql: &str) -> Result<(), String> {
        let sql = sql.trim();
        if sql.is_empty() {
            return Ok(());
        }

        let bind_values = self.bind_values_for_sql(sql);
        let result = if is_query(sql) {
            self.conn
                .query(sql, &bind_values)
                .await
                .map(Execution::Query)
        } else if is_anonymous_plsql(sql) && bind_values.is_empty() {
            self.conn.execute(sql, &[]).await.map(|_| Execution::Plsql)
        } else if starts_plsql_or_create_block(sql) {
            if bind_values.is_empty() {
                self.conn.execute(sql, &[]).await.map(Execution::Dml)
            } else {
                self.execute_plsql_text(sql).await.map(|_| Execution::Plsql)
            }
        } else {
            self.conn
                .execute(sql, &bind_values)
                .await
                .map(Execution::Dml)
        };

        match result {
            Ok(Execution::Query(result)) => {
                print_query_result(&result, self.session_time_zone_offset_minutes);
                if self.feedback {
                    println!();
                    println!(
                        "{} row{} selected.",
                        result.row_count(),
                        if result.row_count() == 1 { "" } else { "s" }
                    );
                    println!();
                }
                Ok(())
            }
            Ok(Execution::Dml(result)) => {
                self.update_session_display_state(sql);
                if self.feedback {
                    println!();
                    println!("{}", feedback_for_statement(sql, result.rows_affected));
                    println!();
                }
                Ok(())
            }
            Ok(Execution::Plsql) => {
                self.update_session_display_state(sql);
                self.drain_dbms_output().await?;
                if self.feedback {
                    println!();
                    println!("PL/SQL procedure successfully completed.");
                    println!();
                }
                Ok(())
            }
            Err(err) => self.handle_statement_error(sql, err),
        }
    }

    async fn execute_plsql_text(&mut self, sql: &str) -> Result<oracle_rs::PlsqlResult, Error> {
        let bind_names = bind_names_for_sql(sql);
        let mut params = Vec::new();
        let mut var_names = Vec::new();
        for name in bind_names {
            let var = self
                .vars
                .get(&name)
                .cloned()
                .unwrap_or_else(|| BindVariable {
                    oracle_type: OracleType::Varchar,
                    buffer_size: 4000,
                    value: None,
                });
            let param = match var.value {
                Some(value) => BindParam::input_output(value, var.buffer_size.max(1)),
                None => BindParam::output(var.oracle_type, var.buffer_size.max(1)),
            };
            params.push(param);
            var_names.push(name);
        }

        let result = self.conn.execute_plsql(sql, &params).await?;
        for (idx, value) in result.out_values.iter().cloned().enumerate() {
            if let Some(name) = var_names.get(idx) {
                self.vars
                    .entry(name.clone())
                    .and_modify(|var| var.value = Some(value.clone()))
                    .or_insert_with(|| BindVariable {
                        oracle_type: infer_oracle_type(&value),
                        buffer_size: default_buffer_size(&value),
                        value: Some(value),
                    });
            }
        }
        Ok(result)
    }

    async fn drain_dbms_output(&mut self) -> Result<(), String> {
        if !self.serveroutput {
            return Ok(());
        }

        loop {
            let result = self
                .conn
                .execute_plsql(
                    "BEGIN DBMS_OUTPUT.GET_LINE(:line, :status); END;",
                    &[
                        BindParam::output(OracleType::Varchar, 32767),
                        BindParam::output(OracleType::Number, 22),
                    ],
                )
                .await
                .map_err(|err| err.to_string())?;

            let status = result
                .out_values
                .get(1)
                .and_then(Value::as_i64)
                .unwrap_or(1);
            if status != 0 {
                break;
            }

            if let Some(line) = result.out_values.first().and_then(Value::as_str) {
                println!("{line}");
            } else {
                println!();
            }
        }

        Ok(())
    }

    fn bind_values_for_sql(&self, sql: &str) -> Vec<Value> {
        bind_names_for_sql(sql)
            .into_iter()
            .map(|name| {
                self.vars
                    .get(&name)
                    .and_then(|var| var.value.clone())
                    .unwrap_or(Value::Null)
            })
            .collect()
    }

    fn handle_statement_error(&self, sql: &str, err: Error) -> Result<(), String> {
        println!();
        match &err {
            Error::OracleError { .. } => print_sqlplus_error(sql, &err),
            _ => println!("{}", sanitize_text(&err.to_string())),
        }
        println!();
        if self.continue_on_error {
            Ok(())
        } else {
            Err(String::new())
        }
    }

    fn update_session_display_state(&mut self, sql: &str) {
        if let Some(offset) = parse_alter_session_time_zone(sql) {
            self.session_time_zone_offset_minutes = Some(offset);
        }
    }
}

enum Execution {
    Query(QueryResult),
    Dml(QueryResult),
    Plsql,
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    if let Err(err) = runtime.block_on(async_main()) {
        if !err.is_empty() {
            eprintln!("{err}");
        }
        std::process::exit(1);
    }
}

async fn async_main() -> Result<(), String> {
    let options = parse_args()?;
    let (config, display_connect) = config_from_options(&options)?;
    println!();
    println!("SQLrs: Release {}", env!("CARGO_PKG_VERSION"));
    println!();

    let conn = Connection::connect_with_config(config)
        .await
        .map_err(|err| format!("ERROR: could not connect to {display_connect}: {err}"))?;
    println!("Connected.");
    println!();

    let mut sqlrs = SqlRs::new(conn).await;
    if let Some(script) = options.script {
        sqlrs.run_file(&script).await?;
    } else {
        return Err("interactive mode is not implemented yet; pass @script.sql".to_string());
    }
    Ok(())
}

fn parse_args() -> Result<Options, String> {
    let mut connect = None;
    let mut script = None;

    for arg in std::env::args().skip(1) {
        if let Some(path) = arg.strip_prefix('@') {
            script = Some(PathBuf::from(path));
        } else if arg == "-h" || arg == "--help" {
            print_usage();
            std::process::exit(0);
        } else if connect.is_none() {
            connect = Some(arg);
        } else if script.is_none() {
            script = Some(PathBuf::from(arg));
        } else {
            return Err(format!("unexpected argument: {arg}"));
        }
    }

    Ok(Options { connect, script })
}

fn print_usage() {
    println!("Usage: sqlrs [user/password@//host:port/service] @script.sql");
    println!();
    println!("If connect is omitted, ORACLE_HOST/ORACLE_PORT/ORACLE_SERVICE/ORACLE_USER/ORACLE_PASSWORD are used.");
}

fn config_from_options(options: &Options) -> Result<(Config, String), String> {
    if let Some(connect) = &options.connect {
        let parsed = parse_connect(connect)?;
        let display = format!("{}:{}/{}", parsed.host, parsed.port, parsed.service);
        return Ok((
            Config::new(
                &parsed.host,
                parsed.port,
                &parsed.service,
                &parsed.user,
                &parsed.password,
            ),
            display,
        ));
    }

    let host = env_or_default("ORACLE_HOST", "localhost");
    let port = env_or_default("ORACLE_PORT", "1521")
        .parse::<u16>()
        .map_err(|err| format!("ORACLE_PORT must be a u16: {err}"))?;
    let service = env_or_default("ORACLE_SERVICE", "FREEPDB1");
    let user = env_or_default("ORACLE_USER", "testuser");
    let password = env_or_default("ORACLE_PASSWORD", "testpass");
    let display = format!("{host}:{port}/{service}");
    Ok((
        Config::new(&host, port, &service, &user, &password),
        display,
    ))
}

struct ParsedConnect {
    user: String,
    password: String,
    host: String,
    port: u16,
    service: String,
}

fn parse_connect(connect: &str) -> Result<ParsedConnect, String> {
    let (credentials, target) = connect
        .split_once('@')
        .ok_or_else(|| "connect string must be user/password@//host:port/service".to_string())?;
    let (user, password) = credentials
        .split_once('/')
        .ok_or_else(|| "connect string must include user/password".to_string())?;
    let target = target.strip_prefix("//").unwrap_or(target);
    let (host_port, service) = target
        .split_once('/')
        .ok_or_else(|| "connect string must include /service".to_string())?;
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        let port = port
            .parse::<u16>()
            .map_err(|err| format!("invalid port in connect string: {err}"))?;
        (host.to_string(), port)
    } else {
        (host_port.to_string(), 1521)
    };

    Ok(ParsedConnect {
        user: user.to_string(),
        password: password.to_string(),
        host,
        port,
        service: service.to_string(),
    })
}

fn env_or_default(name: &str, default_value: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_value.to_string())
}

fn print_query_result(result: &QueryResult, session_time_zone_offset_minutes: Option<i32>) {
    if result.columns.is_empty() {
        for row in &result.rows {
            println!(
                "{}",
                row.values()
                    .iter()
                    .map(format_value)
                    .collect::<Vec<_>>()
                    .join("\t")
            );
        }
        return;
    }

    let mut widths = result
        .columns
        .iter()
        .map(|col| col.name.len().max(1))
        .collect::<Vec<_>>();

    let rows = result
        .rows
        .iter()
        .map(|row| {
            row.values()
                .iter()
                .enumerate()
                .map(|(idx, value)| {
                    let text = format_cell(
                        value,
                        result.columns.get(idx).map(|col| col.oracle_type),
                        session_time_zone_offset_minutes,
                    );
                    if let Some(width) = widths.get_mut(idx) {
                        *width = (*width).max(text.len());
                    }
                    text
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    println!();
    for (idx, col) in result.columns.iter().enumerate() {
        print!("{:width$}", col.name, width = widths[idx]);
        if idx + 1 < result.columns.len() {
            print!(" ");
        }
    }
    println!();
    for (idx, width) in widths.iter().enumerate() {
        print!("{}", "-".repeat(*width));
        if idx + 1 < widths.len() {
            print!(" ");
        }
    }
    println!();
    for row in rows {
        for (idx, text) in row.iter().enumerate() {
            print!("{:width$}", text, width = widths[idx]);
            if idx + 1 < row.len() {
                print!(" ");
            }
        }
        println!();
    }
}

fn format_value(value: &Value) -> String {
    format_cell(value, None, None)
}

fn format_cell(
    value: &Value,
    oracle_type: Option<OracleType>,
    session_time_zone_offset_minutes: Option<i32>,
) -> String {
    match value {
        Value::Null | Value::TypedNull(_) => String::new(),
        Value::String(s) => sanitize_text(s),
        Value::Bytes(bytes) => bytes_to_hex(bytes),
        Value::Date(date) => format_sqlplus_date(
            date.year,
            date.month,
            date.day,
            date.hour,
            date.minute,
            date.second,
        ),
        Value::Timestamp(ts) => {
            if oracle_type == Some(OracleType::TimestampLtz) {
                if let Some(offset_minutes) = session_time_zone_offset_minutes {
                    return format_timestamp_with_offset(ts, offset_minutes);
                }
            }
            format_sqlplus_timestamp(
                ts.year,
                ts.month,
                ts.day,
                ts.hour,
                ts.minute,
                ts.second,
                ts.microsecond,
            )
        }
        Value::Lob(lob) => match lob.as_locator() {
            Some(locator) => format!("<LOB {:?} size={}>", locator.oracle_type(), locator.size()),
            None => format!("<LOB size={:?}>", lob.size()),
        },
        _ => sanitize_text(&value.to_string()),
    }
}

fn format_timestamp_with_offset(
    ts: &oracle_rs::types::OracleTimestamp,
    offset_minutes: i32,
) -> String {
    let Some(date) = chrono::NaiveDate::from_ymd_opt(ts.year, ts.month as u32, ts.day as u32)
    else {
        return format_sqlplus_timestamp(
            ts.year,
            ts.month,
            ts.day,
            ts.hour,
            ts.minute,
            ts.second,
            ts.microsecond,
        );
    };
    let Some(datetime) = date.and_hms_micro_opt(
        ts.hour as u32,
        ts.minute as u32,
        ts.second as u32,
        ts.microsecond,
    ) else {
        return format_sqlplus_timestamp(
            ts.year,
            ts.month,
            ts.day,
            ts.hour,
            ts.minute,
            ts.second,
            ts.microsecond,
        );
    };
    let adjusted = datetime + chrono::Duration::minutes(offset_minutes as i64);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
        adjusted.year(),
        adjusted.month(),
        adjusted.day(),
        adjusted.hour(),
        adjusted.minute(),
        adjusted.second(),
        adjusted.nanosecond() / 1000
    )
}

fn format_sqlplus_date(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        display_year(year),
        month,
        day,
        hour,
        minute,
        second
    )
}

fn format_sqlplus_timestamp(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    microsecond: u32,
) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
        display_year(year),
        month,
        day,
        hour,
        minute,
        second,
        microsecond
    )
}

fn display_year(year: i32) -> i32 {
    if year < 0 {
        year.abs()
    } else {
        year
    }
}

fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter_map(|ch| match ch {
            '\0' => None,
            '\n' | '\r' | '\t' => Some(ch),
            ch if ch.is_control() => None,
            ch => Some(ch),
        })
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn feedback_for_statement(sql: &str, rows_affected: u64) -> String {
    let upper = sql.trim_start().to_ascii_uppercase();
    if upper.starts_with("CREATE MATERIALIZED VIEW") {
        "Materialized view created.".to_string()
    } else if upper.starts_with("CREATE GLOBAL TEMPORARY TABLE")
        || upper.starts_with("CREATE TABLE")
    {
        "Table created.".to_string()
    } else if upper.starts_with("CREATE VIEW") {
        "View created.".to_string()
    } else if upper.starts_with("CREATE OR REPLACE PROCEDURE")
        || upper.starts_with("CREATE PROCEDURE")
    {
        "Procedure created.".to_string()
    } else if upper.starts_with("CREATE OR REPLACE FUNCTION")
        || upper.starts_with("CREATE FUNCTION")
    {
        "Function created.".to_string()
    } else if upper.starts_with("CREATE OR REPLACE PACKAGE") || upper.starts_with("CREATE PACKAGE")
    {
        "Package created.".to_string()
    } else if upper.starts_with("CREATE OR REPLACE TRIGGER") || upper.starts_with("CREATE TRIGGER")
    {
        "Trigger created.".to_string()
    } else if upper.starts_with("CREATE ") {
        "Created.".to_string()
    } else if upper.starts_with("ALTER ") {
        "Session altered.".to_string()
    } else if upper.starts_with("DROP TABLE") {
        "Table dropped.".to_string()
    } else if upper.starts_with("DROP VIEW") {
        "View dropped.".to_string()
    } else if upper.starts_with("DROP MATERIALIZED VIEW") {
        "Materialized view dropped.".to_string()
    } else if upper.starts_with("DROP PROCEDURE") {
        "Procedure dropped.".to_string()
    } else if upper.starts_with("DROP ") {
        "Dropped.".to_string()
    } else if upper.starts_with("COMMIT") {
        "Commit complete.".to_string()
    } else if upper.starts_with("ROLLBACK") {
        "Rollback complete.".to_string()
    } else if upper.starts_with("INSERT ") {
        if rows_affected == 1 {
            "1 row created.".to_string()
        } else {
            format!("{rows_affected} rows created.")
        }
    } else if upper.starts_with("UPDATE ") {
        if rows_affected == 1 {
            "1 row updated.".to_string()
        } else {
            format!("{rows_affected} rows updated.")
        }
    } else if upper.starts_with("DELETE ") {
        if rows_affected == 1 {
            "1 row deleted.".to_string()
        } else {
            format!("{rows_affected} rows deleted.")
        }
    } else if rows_affected == 1 {
        "1 row affected.".to_string()
    } else {
        format!("{rows_affected} rows affected.")
    }
}

fn print_sqlplus_error(sql: &str, err: &Error) {
    let message = format_oracle_error(err);
    let first_line = sql.lines().next().unwrap_or(sql).trim_end();
    println!("{first_line}");
    println!(
        "{}*",
        " ".repeat(error_pointer_column(first_line, &message))
    );
    println!("ERROR at line 1:");
    println!("{message}");
}

fn error_pointer_column(sql_line: &str, message: &str) -> usize {
    if let Some(token) = quoted_oracle_identifier(message) {
        if let Some(pos) = find_case_insensitive(sql_line, &token) {
            return pos;
        }
    }
    if message.contains("ORA-01476") {
        if let Some(pos) = sql_line.find('/') {
            return pos;
        }
    }
    let upper = sql_line.to_ascii_uppercase();
    if upper.starts_with("SELECT ") {
        return "SELECT ".len();
    }
    sql_line.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn quoted_oracle_identifier(message: &str) -> Option<String> {
    let start = message.find('"')?;
    let rest = &message[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_uppercase()
        .find(&needle.to_ascii_uppercase())
}

fn parse_alter_session_time_zone(sql: &str) -> Option<i32> {
    let upper = sql.to_ascii_uppercase();
    if !upper.starts_with("ALTER SESSION SET TIME_ZONE") {
        return None;
    }

    let (_, value) = sql.split_once('=')?;
    let value = strip_trailing_semicolon(value.trim())
        .trim()
        .trim_matches('\'');
    parse_time_zone_offset(value)
}

fn parse_time_zone_offset(value: &str) -> Option<i32> {
    if value.eq_ignore_ascii_case("UTC") || value == "+00:00" || value == "-00:00" {
        return Some(0);
    }
    if value.eq_ignore_ascii_case("Asia/Kolkata") {
        return Some(5 * 60 + 30);
    }
    parse_numeric_time_zone_offset(value)
}

fn parse_numeric_time_zone_offset(value: &str) -> Option<i32> {
    let sign = match value.as_bytes().first().copied()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (hours, minutes) = value[1..].split_once(':')?;
    let hours = hours.parse::<i32>().ok()?;
    let minutes = minutes.parse::<i32>().ok()?;
    Some(sign * (hours * 60 + minutes))
}

fn format_oracle_error(err: &Error) -> String {
    match err {
        Error::OracleError { code, message } => {
            let message = sanitize_text(message).trim().to_string();
            if message.is_empty() {
                format!("ORA-{code:05}")
            } else {
                message
            }
        }
        _ => sanitize_text(&err.to_string()),
    }
}

fn starts_plsql_or_create_block(sql: &str) -> bool {
    let upper = sql.trim_start().to_ascii_uppercase();
    is_anonymous_plsql(sql)
        || upper.starts_with("CREATE OR REPLACE PROCEDURE")
        || upper.starts_with("CREATE PROCEDURE")
        || upper.starts_with("CREATE OR REPLACE FUNCTION")
        || upper.starts_with("CREATE FUNCTION")
        || upper.starts_with("CREATE OR REPLACE PACKAGE")
        || upper.starts_with("CREATE PACKAGE")
        || upper.starts_with("CREATE OR REPLACE TRIGGER")
        || upper.starts_with("CREATE TRIGGER")
}

fn is_anonymous_plsql(sql: &str) -> bool {
    let upper = sql.trim_start().to_ascii_uppercase();
    upper.starts_with("BEGIN") || upper.starts_with("DECLARE")
}

fn is_query(sql: &str) -> bool {
    let upper = sql.trim_start().to_ascii_uppercase();
    upper.starts_with("SELECT") || upper.starts_with("WITH")
}

fn ends_sql_statement(line: &str) -> bool {
    line.trim_end().ends_with(';')
}

fn strip_trailing_semicolon(sql: &str) -> &str {
    sql.trim_end().strip_suffix(';').unwrap_or(sql).trim_end()
}

fn normalize_var_name(name: &str) -> String {
    name.trim().trim_start_matches(':').to_ascii_uppercase()
}

fn parse_sqlplus_type(type_spec: &str) -> (OracleType, u32) {
    let upper = type_spec.to_ascii_uppercase();
    let size = upper
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .and_then(|(num, _)| num.parse::<u32>().ok())
        .unwrap_or(4000);

    if upper.starts_with("NUMBER") {
        (OracleType::Number, 22)
    } else if upper.starts_with("BINARY_DOUBLE") {
        (OracleType::BinaryDouble, 8)
    } else if upper.starts_with("BINARY_FLOAT") {
        (OracleType::BinaryFloat, 4)
    } else if upper.starts_with("RAW") {
        (OracleType::Raw, size)
    } else {
        (OracleType::Varchar, size)
    }
}

fn parse_literal(text: &str) -> Value {
    let trimmed = strip_trailing_semicolon(text.trim()).trim();
    if trimmed.eq_ignore_ascii_case("NULL") {
        Value::Null
    } else if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        Value::String(trimmed[1..trimmed.len() - 1].replace("''", "'"))
    } else if let Ok(value) = trimmed.parse::<i64>() {
        Value::Integer(value)
    } else if let Ok(value) = trimmed.parse::<f64>() {
        Value::Float(value)
    } else {
        Value::String(trimmed.to_string())
    }
}

fn infer_oracle_type(value: &Value) -> OracleType {
    match value {
        Value::Integer(_) | Value::Number(_) => OracleType::Number,
        Value::Float(_) => OracleType::BinaryDouble,
        Value::Bytes(_) => OracleType::Raw,
        _ => OracleType::Varchar,
    }
}

fn default_buffer_size(value: &Value) -> u32 {
    match value {
        Value::String(s) => s.len().max(1) as u32,
        Value::Bytes(bytes) => bytes.len().max(1) as u32,
        Value::Integer(_) | Value::Number(_) => 22,
        Value::Float(_) => 8,
        _ => 4000,
    }
}

fn bind_names_for_sql(sql: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut chars = sql.char_indices().peekable();
    let mut in_single_quote = false;

    while let Some((_, ch)) = chars.next() {
        if ch == '\'' {
            in_single_quote = !in_single_quote;
            continue;
        }
        if in_single_quote || ch != ':' {
            continue;
        }

        let mut name = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || next == '_' || next == '$' || next == '#' {
                name.push(next);
                chars.next();
            } else {
                break;
            }
        }

        if !name.is_empty() {
            names.push(normalize_var_name(&name));
        }
    }

    names
}

fn parse_call(call: &str) -> Result<(String, Vec<String>), String> {
    let open = call
        .find('(')
        .ok_or_else(|| format!("SP2-0734: unsupported EXEC form: {call}"))?;
    let close = call
        .rfind(')')
        .ok_or_else(|| format!("SP2-0734: unsupported EXEC form: {call}"))?;
    let name = call[..open].trim().to_string();
    let args = split_top_level(&call[open + 1..close], ',')
        .into_iter()
        .map(|arg| arg.trim().to_string())
        .filter(|arg| !arg.is_empty())
        .collect();
    Ok((name, args))
}

fn split_top_level(text: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_single_quote = false;

    for (idx, ch) in text.char_indices() {
        match ch {
            '\'' => in_single_quote = !in_single_quote,
            '(' if !in_single_quote => depth += 1,
            ')' if !in_single_quote => depth -= 1,
            _ if ch == delimiter && !in_single_quote && depth == 0 => {
                parts.push(&text[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}
