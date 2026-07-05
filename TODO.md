# TODO

本文件记录当前 `oracle-rs` thin 协议/lib 层和 `sqlrs` SQL*Plus-like
命令行工具的已知问题、未实现内容和后续测试计划。

## P0: Thin 协议 / lib 层

- 实现 CONNECT 阶段的 REDIRECT 包处理。
  - `src/messages/redirect.rs` 已有解析器。
  - `src/connection.rs` 收到 packet type `5` 时仍会返回 `redirect not implemented`。
  - 需要对齐 node-oracledb thin：限制 redirect 次数、解析新地址、断开并重连、重新发送 CONNECT。
  - 这会影响 RAC、SCAN listener、CMAN、云数据库路由等场景。

- 完善 REFUSE 包解析。
  - 当前连接拒绝大多只返回泛化的 `Connection refused by server`。
  - 需要解析 listener 返回的 ORA/TNS 错误，例如 `ORA-12514`、`ORA-12505`。
  - 错误结构中应保留 code、message、原始 payload/诊断信息。

- 继续加固 SQL 错误后的 break/reset 恢复。
  - 目前已覆盖基础 SQL 错误后连接继续可用的场景。
  - 还需要覆盖 PL/SQL 错误、parse error、numeric conversion error、LOB 操作错误、partial fetch 后错误。
  - 协议层应保持连接状态一致，不能让后续 execute/fetch 被上一条错误污染。

- 完整暴露 batch error。
  - 当前已有结构化 `BatchResult.errors` 路径。
  - 继续验证多错误、offset、row count、RETURNING batch、statement cache 交互。
  - 目标是对齐 node-oracledb 的 batch error code/offset/message 行为。

- 完善 server-side piggyback / session state。
  - 现有 `Connection::session_state()` 已保存部分 session state、LTXID、session id。
  - 未知 piggyback opcode 需要在可安全跳过时跳过，并保留 debug/diagnostic 信息。
  - 需要继续补齐 NLS、timezone、transaction/session 状态变化。

- 补齐 OUT / IN OUT 的协议主线。
  - `NUMBER`、字符串、基础 LOB、REF CURSOR、基础 collection 已开始可用。
  - 仍需扩展 Object、collection、nested object、collection of object、associative array。
  - collection 需要更完整的 type descriptor、OID、schema/type metadata。

- 实现 INTERVAL input bind 的更多覆盖。
  - 查询解码和基础 input bind 已实现。
  - 还需要补更多边界值、NULL、array bind、collection 内 interval、PL/SQL OUT/IN OUT interval 测试。

## P1: Thin 协议 / lib 层

- DRCP 支持。
  - `Config::with_drcp()` 目前主要保留 builder API 形状。
  - 需要补 connection class、purity、release mode、session release 语义。

- LOB 边界场景。
  - 已有 CLOB/BLOB/BFILE/temp LOB 基础读写。
  - 需要测试 abstract LOB flag、locator 生命周期、chunk boundary、empty vs null LOB、DML RETURNING LOB。

- statement/cache/cursor 生命周期。
  - >100 行 fetch continuation 已验证。
  - 还需要覆盖 error 后 cursor reuse、implicit result cleanup、close cursor 边界、bind metadata 变化后的 reexecute。

- timestamp/time zone 语义。
  - 当前 thin 层会把部分 timestamp-with-time-zone 归一到驱动内部表示。
  - 若要完全复现 SQL*Plus，需要考虑在 `Value` 中保留原始 offset/region。

- public API 整理。
  - `src/lib.rs` 需要持续导出稳定、自然的 API：`Value::null(OracleType)`、`BindDirection`、LOB、REF CURSOR、collection、batch result。
  - typed-null OUT 的开发体验要保持接近文档示例，不应要求用户理解 TTC 细节。

## P2: Thin 协议 / lib 层

- XMLType 暂不优先处理。
  - 目前可先作为 string-like fallback。
  - 后续再补 metadata、fetch、bind、测试。

- JSON/OSON 暂不优先处理。
  - 基础路径已有，但 edge case 未完全覆盖。
  - 后续再补 interval-in-JSON、更多 OSON node type、node-oracledb parity。

- VECTOR parity。
  - 基础 vector 支持已有。
  - 需要补 23ai、sparse vector、OSON/vector 组合场景。

- Advanced Queuing、CQN、Application Continuity、SODA、XA、sharding。
  - 当前未实现或仅有常量/局部解析。
  - 后续按 node-oracledb thin 行为逐步翻译测试并实现。

## P0: sqlrs

- `sqlrs` 不应修协议层问题。
  - 数据库行为、错误恢复、REDIRECT、OUT 参数、LOB、cursor 等应优先在 `oracle-rs` lib/thin 层修。
  - `sqlrs` 只负责 SQL*Plus 命令解析、脚本执行和输出格式。

- CONNECT/登录行为依赖 lib 层 REDIRECT。
  - lib 实现 REDIRECT 后，`sqlrs` 应通过 `Connection::connect_with_config()` 自动受益。
  - 需要继续补 SQL*Plus 风格的登录失败/错误输出。

- SQL 错误输出。
  - 已有基础 ORA 错误块和多行 SQL 定位格式。
  - 需要补 PL/SQL 编译错误、嵌套 ORA stack、多行 parse error、错误位置 caret/line 兼容。

- `examples/example.sql` 兼容基线。
  - 继续用 `assets/sqlplus/example.sqlplus.out` 作为 SQL*Plus 参考输出。
  - 每次修改 example 脚本后，都要重新生成 SQL*Plus baseline。

## P1: sqlrs

- SQL*Plus 命令覆盖。
  - `COLUMN`
  - `COPY`
  - `DESCRIBE`
  - `SPOOL`
  - `START` / nested `@`
  - `DEFINE` / `UNDEFINE` / substitution variables
  - `SHOW`
  - `EXIT` / `QUIT` with status

- `SET` 命令覆盖。
  - 已有基础 `ECHO`、`FEEDBACK`、`SERVEROUTPUT`。
  - 需要补 `PAGESIZE`、`LINESIZE`、`HEADING`、`TERMOUT`、`VERIFY`、`TIMING`、`SQLPROMPT`、`NUMFORMAT`、NLS display 相关设置。

- 输出兼容。
  - column width、wrapping、分页、重复 heading。
  - 数字右对齐、科学计数法、空行规则。
  - DML/DDL feedback：`COMMENT`、`TRUNCATE`、`CREATE INDEX`、`ALTER TABLE`、package/type creation 等。
  - UTC timestamp / session timezone / NLS 的 SQL*Plus 文本展示。

- `DBMS_OUTPUT` 行为。
  - 基础读取已可用。
  - 需要补 line size、buffer size、禁用/启用行为、错误行为。

- interactive mode。
  - 当前主要面向 `@script.sql`。
  - 需要补 prompt loop、statement buffer、`/` 执行、history、Ctrl-C/break 处理。

## P2: sqlrs

- SQL*Plus diff 测试。
  - 添加 normalized diff，忽略 SID、时间戳、client program name 等易变字段。
  - 用 `example.sql` 做 smoke test，用 focused tests 捕获协议回归。

- 脚本 bind variable。
  - 当前 `VAR`/`PRINT` 是基础实现。
  - 需要补更多 SQL*Plus type syntax、数组、REF CURSOR 打印、LOB 打印。

- 安装和 CI 文档。
  - 补 `cargo install`、环境变量、CI 中执行脚本的示例。

## 测试待办

- 翻译 node-oracledb thin 测试到 Rust integration tests。
  - error recovery / reset
  - batch DML errors
  - REF CURSOR / implicit results
  - LOB read/write/returning
  - object / collection
  - session state / DRCP
  - REDIRECT / REFUSE

- 保持 `examples/example.sql` 广而稳定。
  - 覆盖 NLS、BLOB/CLOB、OUT/IN OUT、临时表、DBMS_OUTPUT、SQL 错误、INTERVAL、大结果集 fetch。
  - 它是兼容性 smoke test，不应替代 focused protocol tests。
