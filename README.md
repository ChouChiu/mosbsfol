# MOSBSFOL — macOS Bull Shit Feature On Linux

> [!CAUTION]
> 免责声明：本项目（包括全部源代码、文档与 README）由 AI（大语言模型）辅助生成，可能存在错误、遗漏或过时信息，仅供参考、学习与兼容性测试，不构成任何形式的保证。
>
> 生成的 macOS 痕迹仅应在自有设备或已获授权的环境中使用；请勿将其用于干扰他人设备、数据或任何非法用途。
>
> 本项目与 Apple Inc. 无任何关联，未获得其认可或背书；文中提及的商标归其各自权利人所有。
>
> 使用本项目所产生的任何直接或间接后果，由使用者自行承担；项目作者与生成内容的 AI 均不承担任何责任。

在 Linux 上复刻 macOS 那些“特有但没什么必要”的文件系统行为。

灵感来自 [awesome-windows-on-linux issue #2](https://github.com/windowix/awesome-windows-on-linux/issues/2)。

当前覆盖六个最典型的 macOS 痕迹：

- `.DS_Store`：Finder 视图状态
- `._文件名` / AppleDouble：Resource Fork、FinderInfo 的侧车文件
- `__MACOSX/`：Finder 风格 ZIP 元数据
- `.plist`：XML / `bplist00`
- `xattr`：quarantine、FinderInfo、ResourceFork、WhereFroms、Tags、Hidden
- 卷根目录痕迹：`.Spotlight-V100`、`.fseventsd`、`.Trashes`、
  `.TemporaryItems`、`.localized`、`.VolumeIcon.icns`、`Icon\r`

特性按标准 Feature-Driven 组织，每个行为对应一个 Cargo feature。
**零第三方 Rust crate 依赖。**

---

## 快速开始

```sh
cargo build --release
alias mosbsfol="$PWD/target/release/mosbsfol"

mosbsfol --help

# 给 U 盘挂载点来一套完整的“被 Mac 插过”痕迹
mosbsfol usb /mnt/usb -r --include-dirs --type-codes
```

也可以安装到 `~/.cargo/bin`：

```sh
cargo install --path .
```

---

## Feature 一览

默认启用全部 feature：

```toml
default = ["dsstore", "appledouble", "maczip", "plist", "xattr", "volumetrace"]
```

| Cargo feature | 命令 | 功能 |
| --- | --- | --- |
| `dsstore` | `dsstore` / `poop` | 生成、解析、清理 Finder 可读的 `.DS_Store` |
| `appledouble` | `usb`* | AppleDouble v2 `._*` 侧车；自动携带 FinderInfo 与 Resource Fork |
| `maczip` | `maczip` | 生成带 `__MACOSX/._*` 条目的 Finder 风格 ZIP |
| `plist` | `plist` | 读写 XML plist 与 `bplist00` 二进制 plist |
| `xattr` | `xattr` | `com.apple.*` xattr：quarantine、FinderInfo、ResourceFork、WhereFroms、Tags、Hidden |
| `volumetrace` | `trace` | 卷根目录痕迹；`usb` 也会自动调用 |

\* `usb` 需要同时启用 `appledouble` + `dsstore`；启用 `volumetrace` 时还会追加卷根目录痕迹。

按需裁剪：

```sh
# 只要 .DS_Store
cargo build --no-default-features --features dsstore

# 只要 plist + xattr
cargo build --no-default-features --features plist,xattr

# USB 全套痕迹
cargo build --no-default-features --features dsstore,appledouble,volumetrace

# 只要 __MACOSX ZIP
cargo build --no-default-features --features maczip
```

---

## 命令示例

### `.DS_Store`

```sh
# 递归生成
mosbsfol poop ~/some/dir -r

# 查看 Finder 记录（文件名、Iloc、bwsp/icvp 等）
mosbsfol dsstore inspect ~/some/dir/.DS_Store

# 清理
mosbsfol dsstore clean ~/some/dir -r
```

### USB / AppleDouble / 卷痕迹

```sh
# ._* + .DS_Store + .Spotlight-V100 + .fseventsd + .Trashes
# + .TemporaryItems + .localized + .VolumeIcon.icns + Icon<CR>
mosbsfol usb /mnt/usb -r --include-dirs --type-codes

# 只操作卷根目录痕迹
mosbsfol trace poop /mnt/usb

# 清理
mosbsfol usb /mnt/usb -r --clean
mosbsfol trace clean /mnt/usb
```

### `__MACOSX` ZIP

```sh
mosbsfol maczip ~/Documents ~/Documents.zip
mosbsfol maczip ~/Documents --dry-run
```

生成合法 stored-ZIP：原始文件 + `__MACOSX/<相对路径>/._<文件名>`。

### plist

```sh
mosbsfol plist write demo.plist name=mosbsfol answer=42 pi=3.14
mosbsfol plist write demo.xml name=mosbsfol --xml
mosbsfol plist read demo.plist
```

### xattr

Linux 没有命名空间时会自动把 `com.apple.*` 映射为 `user.com.apple.*`。

```sh
mosbsfol xattr quarantine ./setup.bin
mosbsfol xattr wherefroms ./setup.bin https://example.com/
mosbsfol xattr finderinfo ./note.txt TEXT MACS
mosbsfol xattr tag ./note.txt red
mosbsfol xattr hide ./note.txt yes
mosbsfol xattr resourcefork ./note.txt deadbeef
mosbsfol xattr comment ./note.txt "来自 Finder 的评论"
mosbsfol xattr list ./note.txt
```

---

## 项目结构

标准 FDD：`core` = 应用核心，`shared` = 共享基础设施，`features/{feature}` = 独立特性。

```text
src/
├── main.rs                        薄启动器：收集参数 -> core::cli
├── lib.rs                         feature 门控的库入口与 re-export
├── core/
│   └── cli.rs                     argv 路由；help 由各 feature 拼装
├── shared/
│   ├── bplist.rs                  bplist00 编解码
│   ├── cli.rs                     参数解析小工具
│   ├── mac.rs                     FInfo/FXInfo、type code、痕迹判定
│   └── util.rs                    UTF-16BE、FourCC、对齐、错误类型
├── features/
│   ├── dsstore/
│   │   ├── format.rs              Bud1 + buddy allocator + B-tree
│   │   ├── finder.rs              Finder 记录生成与递归操作
│   │   └── cli.rs                 dsstore / poop
│   ├── appledouble/
│   │   ├── mod.rs                 AppleDouble v2 `._*`
│   │   └── cli.rs                 usb
│   ├── maczip/
│   │   ├── mod.rs                 __MACOSX 构建
│   │   ├── zip.rs                 最小 stored-ZIP writer（CRC32）
│   │   └── cli.rs                 maczip
│   ├── plist/
│   │   ├── mod.rs                 plist 读写
│   │   └── cli.rs                 plist
│   ├── xattr/
│   │   ├── mod.rs                 xattr 封装 + com.apple.* 生成器
│   │   └── cli.rs                 xattr
│   └── volumetrace/
│       ├── mod.rs                 卷根目录痕迹
│       └── cli.rs                 trace
└── tests/
    └── cli.rs                     feature 门控的端到端测试
```

---

## 开发与测试

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# 64 种 feature 组合全部 check + 抽样 test
./scripts/check-features.sh

# 端到端验收（.DS_Store / usb / plist / xattr / maczip）
./scripts/acceptance.sh
```

交叉验证：

- `bwsp` / `icvp` 内嵌 plist：用 Python `plistlib.loads` 验证
- `.DS_Store`：用独立 Python-dsstore 解析器验证
- `maczip` 输出：用 Python `zipfile` 验证 `__MACOSX/._*` 条目

---

## 已知限制

| 项目 | 说明 |
| --- | --- |
| `.DS_Store` | Apple 未公开格式；按 Mozilla Wiki / `DSStoreFormat.pod` 逆向笔记生成单叶节点 B-tree。排序只是 HFS+ `TN1150` 的近似 |
| AppleDouble | Resource Fork 按原始字节写入，不解析资源管理器内部结构 |
| `__MACOSX` | ZIP 使用 stored（无压缩）方式；结构正确但不是 Finder 逐字节复刻 |
| `.VolumeIcon.icns` / `Icon\r` | 只写入 8 字节合法 ICNS 头占位，不生成真实图标 |
| xattr | 需要文件系统支持 `user` xattr；FAT/exFAT/tmpfs 不支持时应改用 `usb` 的 AppleDouble 路径 |
| `.DS_Store` 文件名 | Linux 非法 UTF-8 文件名会 lossy 转为 U+FFFD |
| 大目录 | 单叶节点设计足够普通目录；接近 1 GiB 的极端目录需扩展为多节点 B-tree |
| `usb --clean` / `trace clean` | 会递归删除 `.Spotlight-V100`、`.Trashes` 等标记目录及其内容；重要数据请先 `--dry-run` |
| `maczip` | 目标 ZIP 已存在时会直接覆盖 |

---

## 参考资料

- [awesome-windows-on-linux issue #2](https://github.com/windowix/awesome-windows-on-linux/issues/2)
- [Mozilla Wiki: DS_Store File Format](https://wiki.mozilla.org/DS_Store_File_Format)
- [Mac-Finder-DSStore: DSStoreFormat.pod](http://search.cpan.org/~wiml/Mac-Finder-DSStore/DSStoreFormat.pod)
- [digi.ninja: fdb](https://digi.ninja/projects/fdb.php)
- [RFC 1740: AppleSingle / AppleDouble](https://www.rfc-editor.org/rfc/rfc1740.txt)
- Apple `plist(5)` / [属性列表 - 维基百科](https://zh.wikipedia.org/wiki/%E5%B1%9E%E6%80%A7%E5%88%97%E8%A1%A8)

---

## 许可证

本项目使用 [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)。

Copyright 2026 ChouChiu。
许可证全文见仓库根目录的 `LICENSE` 文件。
