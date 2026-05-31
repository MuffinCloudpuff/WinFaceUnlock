# Credential Store Design

更新时间：2026-05-31

## 目标

Credential Store 负责保护 Windows 凭据、人脸模板引用、策略和审计记录。

它不是普通配置文件，也不是临时 PoC 存储层。该模块必须长期满足：

1. Windows 密码不以明文字段进入数据库。
2. SQLCipher 只负责数据库整体加密；单个 Windows credential 仍单独加密为 credential blob。
3. master key 必须是真随机 32 字节，并由 Windows DPAPI LocalMachine 保护。
4. credential blob 使用 AES-256-GCM，nonce 来自 Windows CSPRNG。
5. credential blob AAD 绑定 `user_id + credential_ref`，避免换绑复用。
6. 审计日志只记录事件类型、结果和脱敏 detail code，不记录密码、完整 token 或完整人脸图像。

## 当前模块边界

```text
credential_store/
  master_key.rs        # 32 字节 master key，drop 时 zeroize
  secure_random.rs     # Windows CSPRNG adapter
  key_protector.rs     # DPAPI LocalMachine protect/unprotect
  key_file.rs          # protected master key 文件格式
  credential_blob/
    secret.rs          # 内存中的敏感 secret，drop 时 zeroize
    format.rs          # credential blob magic/version/algorithm/nonce/ciphertext
    protector.rs       # AES-256-GCM 加解密和 AAD 绑定
  persistence/
    records.rs         # 持久化 record 类型
    schema.rs          # SQLCipher schema 和迁移语句
    migration.rs       # schema version / migration contract
    repository.rs      # StoreRepository trait
  file_store.rs        # 当前最小内存 store，用于 master key 和 blob contract 测试
```

## Schema 原则

当前 schema 固定为：

```text
schema_metadata
credentials
policies
users
face_templates
user_face_templates
audit_log
```

设计要点：

1. `credentials` 只保存 `encrypted_blob_bytes`，不保存 password / user_pwd / plaintext 字段。
2. `users` 保存 `credential_ref` 和 `policy_id`，不直接保存 `face_template_ref`。
3. `face_templates` 保存加密模板。
4. `user_face_templates` 负责用户和人脸模板的多对多关系，避免一开始把模型限制成一人一脸。
5. 策略字段使用明确语义，如 `liveness_requirement`、`failure_limit_before_cooldown`，不用含糊 `enabled` / `success` / `flag`。
6. 审计结果使用 `event_outcome`，不用裸 bool。

## SQLCipher 接入路线

Rust 侧推荐路线：

```text
rusqlite
-> libsqlite3-sys
-> vcpkg-provided SQLCipher
```

当前项目固定使用 `vcpkg.json` 管理 SQLCipher 原生依赖。默认不使用
`bundled-sqlcipher-vendored-openssl`，原因是 OpenSSL vendored build 在 Windows
上额外依赖完整 Perl 环境，容易污染 Rust 构建链路。

```powershell
vcpkg install --triplet x64-windows
```

Cargo 构建时需要设置：

```powershell
$env:VCPKG_ROOT = 'D:\tools\vcpkg'
$env:VCPKGRS_DYNAMIC = '1'
```

后续接入时必须满足：

1. 数据库打开后立即执行 SQLCipher key 设置。
2. `PRAGMA foreign_keys = ON`。
3. migration 在事务内执行。
4. repository adapter 只负责 SQL 映射，不负责 DPAPI、AES-GCM 或业务策略。
5. SQLCipher adapter 必须有集成测试，测试不应写入真实用户目录。

## 下一步

1. 安装/确认 MSVC、CMake、Ninja、vcpkg。
2. 选择 SQLCipher 编译路线：优先 vcpkg；如不可行，再评估 `bundled-sqlcipher-vendored-openssl`。
3. 新增 `persistence/sqlcipher_repository.rs`，实现 `StoreRepository`。
4. 增加迁移集成测试和加密数据库打开测试。
