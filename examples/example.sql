SET ECHO ON
SET FEEDBACK ON
SET SERVEROUTPUT ON
SET LONG 1000000
SET LONGCHUNKSIZE 32767
SET LINESIZE 200
SET PAGESIZE 100
WHENEVER SQLERROR CONTINUE

PROMPT =========================================================
PROMPT 0. Environment
PROMPT =========================================================

SELECT USER FROM dual;
SELECT * FROM v$version WHERE ROWNUM = 1;
SELECT sys_context('userenv', 'instance_name') AS instance_name FROM dual;
SELECT sys_context('userenv', 'sid') AS sid FROM dual;

SELECT parameter, value
FROM nls_database_parameters
WHERE parameter IN (
  'NLS_CHARACTERSET',
  'NLS_NCHAR_CHARACTERSET'
)
ORDER BY parameter;

ALTER SESSION SET NLS_DATE_FORMAT = 'YYYY-MM-DD HH24:MI:SS';
ALTER SESSION SET NLS_TIMESTAMP_FORMAT = 'YYYY-MM-DD HH24:MI:SS.FF6';
ALTER SESSION SET NLS_TIMESTAMP_TZ_FORMAT = 'YYYY-MM-DD HH24:MI:SS.FF6 TZH:TZM';
ALTER SESSION SET NLS_NUMERIC_CHARACTERS = '.,';


PROMPT =========================================================
PROMPT 1. Clean old objects
PROMPT =========================================================

BEGIN
  EXECUTE IMMEDIATE 'DROP MATERIALIZED VIEW mattest2';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE NOT IN (-12003, -942) THEN
      DBMS_OUTPUT.PUT_LINE('drop mattest2 ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP VIEW ttv';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -942 THEN
      DBMS_OUTPUT.PUT_LINE('drop ttv ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP TABLE typetest1 PURGE';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -942 THEN
      DBMS_OUTPUT.PUT_LINE('drop typetest1 ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP TABLE typetest2 PURGE';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -942 THEN
      DBMS_OUTPUT.PUT_LINE('drop typetest2 ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP TABLE nodb_conn_dept1 PURGE';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -942 THEN
      DBMS_OUTPUT.PUT_LINE('drop nodb_conn_dept1 ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP TABLE nodb_conn_emp4 PURGE';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -942 THEN
      DBMS_OUTPUT.PUT_LINE('drop nodb_conn_emp4 ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP TABLE nodb_conn_emp5 PURGE';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -942 THEN
      DBMS_OUTPUT.PUT_LINE('drop nodb_conn_emp5 ignored: ' || SQLERRM);
    END IF;
END;
/

BEGIN
  EXECUTE IMMEDIATE 'DROP PROCEDURE nodb_bindingtest';
EXCEPTION
  WHEN OTHERS THEN
    IF SQLCODE != -4043 THEN
      DBMS_OUTPUT.PUT_LINE('drop procedure ignored: ' || SQLERRM);
    END IF;
END;
/


PROMPT =========================================================
PROMPT 2. Basic table/query/bind test from node-oracledb style
PROMPT =========================================================

CREATE TABLE nodb_conn_dept1 (
  department_id   NUMBER,
  department_name VARCHAR2(20)
);

INSERT INTO nodb_conn_dept1 VALUES (40, 'Human Resources');
INSERT INTO nodb_conn_dept1 VALUES (20, 'Marketing');

COMMENT ON TABLE nodb_conn_dept1 IS
  'This is a table with information about various departments';

COMMIT;

VAR id NUMBER
EXEC :id := 40

SELECT department_id, department_name
FROM nodb_conn_dept1
WHERE department_id = :id;

EXEC :id := 20

SELECT department_id, department_name
FROM nodb_conn_dept1
WHERE department_id = :id;


PROMPT =========================================================
PROMPT 3. PL/SQL IN / IN OUT / OUT bind behavior
PROMPT =========================================================

CREATE OR REPLACE PROCEDURE nodb_bindingtest (
  p_in    IN     VARCHAR2,
  p_inout IN OUT VARCHAR2,
  p_out   OUT    VARCHAR2
) AS
BEGIN
  p_out := p_in || ' ' || p_inout;
END;
/

VAR io VARCHAR2(100)
VAR o  VARCHAR2(100)

EXEC :io := 'Turing'
EXEC nodb_bindingtest('Alan', :io, :o)

PRINT io
PRINT o


PROMPT =========================================================
PROMPT 4. Statement cache style repeated SQL
PROMPT =========================================================

CREATE TABLE nodb_conn_emp4 (
  id   NUMBER,
  name VARCHAR2(4000)
);

INSERT INTO nodb_conn_emp4 VALUES (1001, 'Chris Jones');
INSERT INTO nodb_conn_emp4 VALUES (1002, 'Tom Kyte');
INSERT INTO nodb_conn_emp4 VALUES (2001, 'Karen Morton');
COMMIT;

VAR num NUMBER
VAR str VARCHAR2(4000)

EXEC :num := 1003; :str := 'Robyn Sands'
INSERT INTO nodb_conn_emp4 VALUES (:num, :str);

EXEC :num := 1004; :str := 'Bryant Lin'
INSERT INTO nodb_conn_emp4 VALUES (:num, :str);

EXEC :num := 1005; :str := 'Patrick Engebresson'
INSERT INTO nodb_conn_emp4 VALUES (:num, :str);

COMMIT;

SELECT id, name FROM nodb_conn_emp4 ORDER BY id;


PROMPT =========================================================
PROMPT 5. Transaction commit / rollback basic behavior
PROMPT =========================================================

CREATE TABLE nodb_conn_emp5 (
  id   NUMBER,
  name VARCHAR2(4000)
);

INSERT INTO nodb_conn_emp5 VALUES (1001, 'Tom Kyte');
INSERT INTO nodb_conn_emp5 VALUES (1002, 'Karen Morton');
COMMIT;

INSERT INTO nodb_conn_emp5 VALUES (1003, 'Patrick Engebresson');
SELECT COUNT(*) AS count_before_rollback FROM nodb_conn_emp5;
ROLLBACK;
SELECT COUNT(*) AS count_after_rollback FROM nodb_conn_emp5;

INSERT INTO nodb_conn_emp5 VALUES (1003, 'Patrick Engebresson');
COMMIT;
SELECT COUNT(*) AS count_after_commit FROM nodb_conn_emp5;


PROMPT =========================================================
PROMPT 6. oracle_fdw remote type table, no GIS
PROMPT =========================================================

CREATE TABLE typetest1 (
  id  NUMBER(5) CONSTRAINT typetest1_pkey PRIMARY KEY,
  c   CHAR(10 CHAR),
  nc  NCHAR(10),
  vc  VARCHAR2(10 CHAR),
  nvc NVARCHAR2(10),
  lc  CLOB,
  lnc NCLOB,
  r   RAW(10),
  u   RAW(16),
  lb  BLOB,
  lr  LONG RAW,
  b   NUMBER(1),
  num NUMBER(7,5),
  fl  BINARY_FLOAT,
  db  BINARY_DOUBLE,
  d   DATE,
  ts  TIMESTAMP WITH TIME ZONE,
  ids INTERVAL DAY TO SECOND,
  iym INTERVAL YEAR TO MONTH
) SEGMENT CREATION IMMEDIATE;

CREATE VIEW ttv AS
SELECT id, vc FROM typetest1;

CREATE TABLE typetest2 (
  id  NUMBER(5) CONSTRAINT typetest2_pkey PRIMARY KEY,
  ts1 TIMESTAMP WITH LOCAL TIME ZONE,
  ts2 TIMESTAMP WITH LOCAL TIME ZONE,
  ts3 TIMESTAMP WITH LOCAL TIME ZONE
) SEGMENT CREATION IMMEDIATE;


PROMPT =========================================================
PROMPT 7. Type insertion tests
PROMPT =========================================================

INSERT INTO typetest1 (
  id, c, nc, vc, nvc, lc, lnc, r, u, lb, lr,
  b, num, fl, db, d, ts, ids, iym
) VALUES (
  1,
  'fixed char',
  N'natl char',
  'varlena',
  N'natl var',
  TO_CLOB('character large object'),
  TO_NCLOB(N'character national large object'),
  HEXTORAW('DEADBEEF'),
  HEXTORAW('055E26FAF1D8771FE0531645990ADD93'),
  TO_BLOB(HEXTORAW('DEADBEEF')),
  HEXTORAW('DEADBEEF'),
  1,
  3.14159,
  3.14159f,
  3.14159d,
  DATE '1968-10-20',
  TO_TIMESTAMP_TZ('2009-01-26 15:02:54.893532 -08:00',
                  'YYYY-MM-DD HH24:MI:SS.FF6 TZH:TZM'),
  INTERVAL '1 02:00:30.000001' DAY TO SECOND,
  INTERVAL '-0-6' YEAR TO MONTH
);

INSERT INTO typetest1 (
  id, c, nc, vc, nvc, lc, lnc, r, u, lb, lr,
  b, num, fl, db, d, ts, ids, iym
) VALUES (
  2,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  HEXTORAW('00'),
  HEXTORAW('00000000000000000000000000000000'),
  EMPTY_BLOB(),
  HEXTORAW('00'),
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL,
  NULL
);

INSERT INTO typetest1 (
  id, c, nc, vc, nvc, lc, lnc, r, u, lb, lr,
  b, num, fl, db, d, ts, ids, iym
) VALUES (
  3,
  'short',
  N'short',
  'short',
  N'short',
  TO_CLOB('short'),
  TO_NCLOB(N'short'),
  HEXTORAW('DEADF00D'),
  HEXTORAW('0560EE342EF91137E0531645990AC874'),
  TO_BLOB(HEXTORAW('DEADF00D')),
  HEXTORAW('DEADF00D'),
  0,
  -2.71828,
  -2.71828f,
  -2.71828d,
  TO_DATE('0044-03-15 BC', 'YYYY-MM-DD BC'),
  TO_TIMESTAMP_TZ('0044-03-15 12:00:00 -00:00 BC',
                  'YYYY-MM-DD HH24:MI:SS TZH:TZM BC'),
  INTERVAL '-2 12:30:00' DAY TO SECOND,
  INTERVAL '-2-6' YEAR TO MONTH
);

COMMIT;


PROMPT =========================================================
PROMPT 8. Type readback / metadata-visible behavior
PROMPT =========================================================

SELECT
  id,
  c,
  nc,
  vc,
  nvc,
  DBMS_LOB.GETLENGTH(lc)  AS lc_len,
  DBMS_LOB.GETLENGTH(lnc) AS lnc_len,
  RAWTOHEX(r) AS r_hex,
  RAWTOHEX(u) AS u_hex,
  DBMS_LOB.GETLENGTH(lb) AS lb_len,
  b,
  num,
  fl,
  db,
  d,
  ts,
  ids,
  iym
FROM typetest1
ORDER BY id;

SELECT column_name, data_type, char_length, data_precision, data_scale, nullable
FROM user_tab_columns
WHERE table_name = 'TYPETEST1'
ORDER BY column_id;


PROMPT =========================================================
PROMPT 9. TIMESTAMP WITH LOCAL TIME ZONE
PROMPT =========================================================

INSERT INTO typetest2 (id, ts1, ts2, ts3) VALUES (
  1,
  FROM_TZ(CAST(TIMESTAMP '2002-08-01 00:00:00' AS TIMESTAMP), 'UTC'),
  FROM_TZ(CAST(TIMESTAMP '2002-08-01 00:00:00' AS TIMESTAMP), 'UTC'),
  FROM_TZ(CAST(TIMESTAMP '2002-08-01 00:00:00' AS TIMESTAMP), 'UTC')
);

ALTER SESSION SET TIME_ZONE = 'UTC';

INSERT INTO typetest2 (id, ts1, ts2, ts3) VALUES (
  2,
  TIMESTAMP '2020-12-31 00:00:00',
  TIMESTAMP '2020-12-31 00:00:00',
  TIMESTAMP '2020-12-31 00:00:00'
);

ALTER SESSION SET TIME_ZONE = 'Asia/Kolkata';

INSERT INTO typetest2 (id, ts1, ts2, ts3) VALUES (
  3,
  TIMESTAMP '2020-12-31 00:00:00',
  TIMESTAMP '2020-12-31 00:00:00',
  TIMESTAMP '2020-12-31 00:00:00'
);

COMMIT;

SELECT id, ts1, ts2, ts3 FROM typetest2 ORDER BY id;


PROMPT =========================================================
PROMPT 10. INTERVAL direct select
PROMPT =========================================================

SELECT INTERVAL '10-2' YEAR TO MONTH AS iym FROM dual;
SELECT INTERVAL '11 10:09:08.555' DAY TO SECOND AS ids FROM dual;


PROMPT =========================================================
PROMPT 11. Error behavior and recovery after bad execute
PROMPT =========================================================

BEGIN
  RAISE_APPLICATION_ERROR(-20000, 'application error raised');
END;
/

BEGIN
  NULL;
END;
/

SELECT y FROM dual;
SELECT 1 + 1 AS after_bad_execute FROM dual;

SELECT 1e126 FROM dual;
SELECT 1 / 0 FROM dual;
SELECT NaN FROM dual;


PROMPT =========================================================
PROMPT 12. Session parameter visibility
PROMPT =========================================================

SELECT machine, osuser, terminal, program
FROM v$session
WHERE sid = SYS_CONTEXT('USERENV', 'SID');

SELECT client_driver
FROM v$session_connect_info
WHERE sid = SYS_CONTEXT('USERENV', 'SID');


PROMPT =========================================================
PROMPT 13. Materialized view and stats
PROMPT =========================================================

CREATE MATERIALIZED VIEW mattest2 REFRESH COMPLETE AS
SELECT id, ts1, ts2, ts3 FROM typetest2;

BEGIN
  DBMS_STATS.GATHER_TABLE_STATS(USER, 'TYPETEST1', NULL, 100);
  DBMS_STATS.GATHER_TABLE_STATS(USER, 'TYPETEST2', NULL, 100);
END;
/

SELECT table_name, num_rows
FROM user_tables
WHERE table_name IN ('TYPETEST1', 'TYPETEST2')
ORDER BY table_name;

PROMPT DONE