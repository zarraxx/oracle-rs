# oracle-rs

A pure Rust Oracle Database thin driver. It speaks the Oracle TNS/TTC protocol
directly and does not require OCI, ODPI-C, SQL*Plus, or Instant Client for normal
driver usage.

[![Crates.io](https://img.shields.io/crates/v/oracle-rs.svg)](https://crates.io/crates/oracle-rs)
[![Documentation](https://docs.rs/oracle-rs/badge.svg)](https://docs.rs/oracle-rs)
[![License](https://img.shields.io/crates/l/oracle-rs.svg)](LICENSE-APACHE)
[![Build Status](https://github.com/stiang/oracle-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/stiang/oracle-rs/actions/workflows/rust.yml)

## Current Status

This crate is under active development. The thin protocol implementation now
covers the core paths needed for normal SQL, PL/SQL, bind variables, OUT
parameters, LOBs, REF CURSORs, batch DML, SQL error recovery, and large fetches.

Known areas that are still incomplete include REDIRECT handling during connect,
full XMLType support, full JSON/OSON edge-case coverage, AQ, CQN, sharding, XA,
SODA, Application Continuity, and complete object type support for deeply nested
UDTs.

## Features

- Pure Rust Oracle thin protocol implementation.
- Async/await on Tokio.
- TCP and TLS/wallet connection support.
- Fast authentication support with fallback to regular authentication.
- SQL error recovery using break/reset marker handling.
- SELECT, DML, DDL, PL/SQL, commits, rollbacks, savepoints.
- Statement caching.
- Fetch continuation for result sets larger than the initial fetch size.
- PL/SQL IN, OUT, and IN OUT binds via `BindParam` and `execute_with_binds`.
- Typed NULL binds with `Value::null(OracleType::...)`.
- REF CURSOR OUT binds and cursor fetching.
- LOB locators, CLOB/BLOB read/write, temporary LOBs, and BFILE helpers.
- Batch DML with array row counts and structured batch errors.
- Session state exposed from server piggyback messages.
- INTERVAL query decoding and input bind encoding.
- Experimental JSON and VECTOR support.
- `sqlrs`, a SQL*Plus-like command-line script runner built on this crate.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
oracle-rs = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Basic usage:

```rust
use oracle_rs::{Config, Connection, Value};

#[tokio::main]
async fn main() -> oracle_rs::Result<()> {
    let config = Config::new("localhost", 1521, "FREEPDB1", "scott", "tiger");
    let conn = Connection::connect_with_config(config).await?;

    let result = conn
        .query(
            "SELECT empno, ename FROM emp WHERE deptno = :1 ORDER BY empno",
            &[Value::Integer(20)],
        )
        .await?;

    for row in &result.rows {
        let empno = row.get_i64(0).unwrap_or_default();
        let ename = row.get_string(1).unwrap_or("");
        println!("{empno}: {ename}");
    }

    conn.close().await?;
    Ok(())
}
```

## Connections

```rust
use oracle_rs::{Config, Connection};

let config = Config::new("hostname", 1521, "service_name", "username", "password");
let conn = Connection::connect_with_config(config).await?;
```

TLS using system roots:

```rust
let config = Config::new("hostname", 2484, "service_name", "username", "password")
    .with_tls()?;
let conn = Connection::connect_with_config(config).await?;
```

TLS using a PEM wallet:

```rust
let config = Config::new("hostname", 2484, "service_name", "username", "password")
    .with_wallet("/path/to/wallet", Some("wallet_password"))?;
let conn = Connection::connect_with_config(config).await?;
```

Statement cache size:

```rust
let config = Config::new("hostname", 1521, "service_name", "username", "password")
    .with_statement_cache_size(100);
```

DRCP descriptor strings can be used, but `Config::with_drcp()` currently only
keeps the builder shape and does not yet store extra DRCP options.

## SQL Execution

SELECT:

```rust
use oracle_rs::Value;

let result = conn
    .query(
        "SELECT department_id, department_name FROM departments WHERE department_id = :1",
        &[Value::Integer(10)],
    )
    .await?;

if let Some(row) = result.first() {
    println!("{:?}", row.values());
}
```

DML:

```rust
let result = conn
    .execute(
        "INSERT INTO users (id, name) VALUES (:1, :2)",
        &[Value::Integer(1), Value::String("Alice".to_string())],
    )
    .await?;

println!("rows affected: {}", result.rows_affected);
conn.commit().await?;
```

Transactions:

```rust
conn.savepoint("before_update").await?;
conn.execute(
    "UPDATE accounts SET balance = balance - :1 WHERE id = :2",
    &[Value::Float(50.0), Value::Integer(1)],
)
.await?;
conn.rollback_to_savepoint("before_update").await?;
conn.commit().await?;
```

## PL/SQL Binds

OUT and IN OUT binds:

```rust
use oracle_rs::{BindDirection, OracleType, Value};

let mut total = Value::null(OracleType::Number);
let mut label = Value::String("base".to_string());
let mut amount = Value::Integer(21);

conn.execute_with_binds(
    "BEGIN :total := :amount * 2; :label := :label || '-done'; END;",
    &mut [
        ("total", &mut total, BindDirection::Out),
        ("amount", &mut amount, BindDirection::In),
        ("label", &mut label, BindDirection::InOut),
    ],
)
.await?;

assert_eq!(total.as_i64(), Some(42));
assert_eq!(label.as_str(), Some("base-done"));
```

REF CURSOR OUT bind:

```rust
use oracle_rs::{BindDirection, OracleType, Value};

let mut cursor = Value::null(OracleType::Cursor);

conn.execute_with_binds(
    "BEGIN OPEN :cursor FOR SELECT 1 AS id, 'Ada' AS name FROM dual; END;",
    &mut [("cursor", &mut cursor, BindDirection::Out)],
)
.await?;

let cursor = cursor.as_cursor().expect("REF CURSOR");
let rows = conn.fetch_cursor(cursor).await?;
```

Collections can be used through `DbObjectType`, `DbObject`, and
`BindParam::input_collection()` / `BindParam::output_collection()`. Basic VARRAY
and nested table paths are implemented; complex nested object graphs are still
limited.

## Batch DML

```rust
use oracle_rs::{BatchBuilder, Value};

let batch = BatchBuilder::new("INSERT INTO users (id, name) VALUES (:1, :2)")
    .add_row(vec![Value::Integer(1), Value::String("Alice".to_string())])
    .add_row(vec![Value::Integer(2), Value::String("Bob".to_string())])
    .with_row_counts()
    .build();

let result = conn.execute_batch(&batch).await?;
println!("total rows affected: {}", result.total_rows_affected);
println!("row counts: {:?}", result.row_counts);
```

Batch errors:

```rust
let batch = BatchBuilder::new("INSERT INTO users (id, name) VALUES (:1, :2)")
    .add_row(vec![Value::Integer(1), Value::String("ok".to_string())])
    .add_row(vec![Value::Integer(1), Value::String("duplicate".to_string())])
    .with_batch_errors()
    .build();

let result = conn.execute_batch(&batch).await?;
for err in &result.errors {
    println!("row {} failed: ORA-{:05} {}", err.row_index, err.code, err.message);
}
```

## LOBs

Queries return CLOB/BLOB values as `Value::Lob(LobValue::Locator(...))` for
normal locator-based access. Use connection helpers to read or write the locator.

```rust
use oracle_rs::{LobValue, Value};

let result = conn.query("SELECT document FROM files WHERE id = :1", &[Value::Integer(1)]).await?;

if let Some(Value::Lob(LobValue::Locator(locator))) = result.rows[0].get(0) {
    let text = conn.read_clob(locator).await?;
    println!("{text}");
}
```

Temporary LOBs are supported:

```rust
use oracle_rs::OracleType;

let locator = conn.create_temp_lob(OracleType::Clob).await?;
conn.write_clob(&locator, 1, "hello").await?;
let text = conn.read_clob(&locator).await?;
```

## Data Types

| Oracle Type | Rust value |
|-------------|------------|
| NUMBER | `Value::Integer`, `Value::Number`, `Value::Float` |
| VARCHAR2, CHAR, LONG | `Value::String` |
| DATE | `Value::Date(OracleDate)` |
| TIMESTAMP / TIMESTAMP TZ / TIMESTAMP LTZ | `Value::Timestamp(OracleTimestamp)` |
| INTERVAL YEAR TO MONTH | `Value::IntervalYM(OracleIntervalYM)` |
| INTERVAL DAY TO SECOND | `Value::IntervalDS(OracleIntervalDS)` |
| RAW, LONG RAW | `Value::Bytes` |
| CLOB, NCLOB, BLOB, BFILE | `Value::Lob` |
| REF CURSOR | `Value::Cursor(RefCursor)` |
| BOOLEAN | `Value::Boolean` |
| collections / UDTs | `Value::Collection(DbObject)` |
| JSON | `Value::Json`, partial support |
| VECTOR | `Value::Vector`, experimental |
| XMLTYPE | currently treated as string-like fallback, not full XMLType |

Timestamp note: thin protocol timestamp-with-time-zone values are currently
normalized to the server/UTC-style timestamp representation used by the driver;
preserving and displaying the original textual time-zone offset is not yet fully
implemented.

## Session State

Server-side piggyback messages are parsed and exposed through:

```rust
let state = conn.session_state();
println!("sid: {:?}", state.session_id);
println!("serial: {:?}", state.serial_number);
println!("ltxid: {:?}", state.ltxid);
```

This is useful for debugging session identity and future DRCP/Application
Continuity work.

## sqlrs

`sqlrs` is a small SQL*Plus-like command-line script runner built on `oracle-rs`.
It is intended for compatibility testing and practical script execution without
Oracle Instant Client.

Run with an EZConnect-style string:

```sh
cargo run --bin sqlrs -- SCOTT/tiger@//192.168.11.24:1521/FREEPDB1 @examples/example.sql
```

Or use environment variables:

```sh
ORACLE_HOST=192.168.11.24 \
ORACLE_PORT=1521 \
ORACLE_SERVICE=FREEPDB1 \
ORACLE_USER=SCOTT \
ORACLE_PASSWORD=tiger \
cargo run --bin sqlrs -- @examples/example.sql
```

Currently supported SQL*Plus-style script features include:

- `SET ECHO`, `SET FEEDBACK`, and `SET SERVEROUTPUT`.
- `PROMPT`.
- `WHENEVER SQLERROR CONTINUE`.
- `VARIABLE` / `VAR` and `PRINT`.
- Simple `EXEC` / `EXECUTE` procedure calls.
- PL/SQL blocks terminated by `/`.
- DBMS_OUTPUT fetching.
- SQL*Plus-like DDL/DML feedback for common statements.
- Continued execution after recoverable SQL errors.

Known `sqlrs` limitations:

- Interactive mode is not implemented; pass `@script.sql`.
- `COLUMN`, `COPY`, pagination, wrapping, and exact SQL*Plus formatting are not
  complete.
- Output is intentionally getting closer to SQL*Plus but is not byte-for-byte
  compatible yet.

The repository includes `examples/example.sql` and the SQL*Plus reference output
under `assets/sqlplus/example.sqlplus.out` for comparison.

## Protocol Gaps

Important thin protocol areas that still need work:

- CONNECT-time REDIRECT packet handling for RAC/SCAN/CMAN.
- More detailed REFUSE error parsing.
- Full DRCP configuration support.
- More complete server-side piggyback opcode coverage.
- Full XMLType implementation.
- Full JSON/OSON edge-case coverage.
- Advanced Queuing, Continuous Query Notification, sharding, XA, SODA, and
  Application Continuity.

## Minimum Oracle Version

Oracle Database 12c Release 1 (12.1) or later is the target baseline. Some
features require newer database versions:

- Native BOOLEAN: Oracle 23c.
- JSON type: Oracle 21c and later.
- VECTOR type: Oracle 23ai.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Author

[Stian Grytøyr](https://github.com/stiang)

## Contributing

Contributions are welcome. Please feel free to submit a Pull Request.
