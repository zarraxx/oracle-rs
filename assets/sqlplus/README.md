# SQL*Plus Baselines

This directory stores SQL*Plus output captured from scripts in `examples/`.

`example.sqlplus.out` was generated with Oracle Instant Client SQL*Plus 19.31
against Oracle AI Database 26ai Free 23.26:

```sh
LD_LIBRARY_PATH=/tmp/oracle-rs-example/compat:/home/zarra/opt/instantclient/instantclient_19_31 \
  /home/zarra/opt/instantclient/instantclient_19_31/sqlplus \
  -L SCOTT/tiger@//192.168.11.24:1521/FREEPDB1 \
  @examples/example.sql \
  > assets/sqlplus/example.sqlplus.out 2>&1
```

The `/tmp/oracle-rs-example/compat` path supplied a local `libaio.so.1`
compatibility symlink for this host.
