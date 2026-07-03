use oracle_rs::{Config, Connection};

fn env_or_default(name: &str, default_value: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_value.to_string())
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let result = runtime.block_on(async {
        let host = env_or_default("ORACLE_HOST", "192.168.11.24");
        let port = env_or_default("ORACLE_PORT", "1521")
            .parse::<u16>()
            .expect("ORACLE_PORT must be a u16");
        let database = env_or_default("ORACLE_DATABASE", "FREEPDB1");
        let user = env_or_default("ORACLE_USER", "SCOTT");
        let password = env_or_default("ORACLE_PASSWORD", "tiger");

        println!("connecting to {}:{}/{} as {}", host, port, database, user);

        let config = Config::new(host, port, database, user, password);
        let conn = Connection::connect_with_config(config).await?;

        println!("query[1]=SELECT DBMS_DB_VERSION.VERSION, DBMS_DB_VERSION.RELEASE FROM DUAL");
        match conn
            .query(
                "SELECT DBMS_DB_VERSION.VERSION, DBMS_DB_VERSION.RELEASE FROM DUAL",
                &[],
            )
            .await
        {
            Ok(result) => println!("query[1].ok rows={}", result.row_count()),
            Err(err) => println!("query[1].err type={:?} display={}", err, err),
        }

        println!("query[2]=SELECT USER FROM DUAL");
        match conn.query("SELECT USER FROM DUAL", &[]).await {
            Ok(result) => {
                println!("query[2].ok rows={}", result.row_count());
                for (idx, row) in result.rows.iter().enumerate() {
                    println!("  row[{idx}]={:?}", row.get(0));
                }
            }
            Err(err) => println!("query[2].err type={:?} display={}", err, err),
        }

        Ok::<(), oracle_rs::Error>(())
    });

    if let Err(err) = result {
        eprintln!("probe failed: {err:?}");
        std::process::exit(1);
    }
}
