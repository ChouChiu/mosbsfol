# MOSBSFOL — macOS Bull Shit Feature On Linux

> 在 Linux 上复刻 macOS 那些“特有但没什么必要”的文件系统行为。
> 灵感来自 [awesome-windows-on-linux issue #2](https://github.com/windowix/awesome-windows-on-linux/issues/2)。

> [!CAUTION]
> 本项目（包括全部源代码、文档与 README）由 AI（大语言模型）辅助生成，可能存在错误、遗漏或过时信息，仅供参考、学习与兼容性测试，不构成任何形式的保证。
>
> 生成的 macOS 痕迹仅应在自有设备或已获授权的环境中使用；请勿将其用于干扰他人设备、数据或任何非法用途。
>
> 本项目与 Apple Inc. 无任何关联，未获得其认可或背书；文中提及的商标归其各自权利人所有。
>
> 使用本项目所产生的任何直接或间接后果，由使用者自行承担；项目作者与生成内容的 AI 均不承担任何责任。

---

## 目录

- [项目简介](#项目简介)
- [快速开始](#快速开始)
- [Feature 一览](#feature-一览)
- [命令示例](#命令示例)
- [autopoop：自动拉屎](#autopoop自动拉屎)
- [已知限制](#已知限制)
- [参考资料](#参考资料)
- [许可证](#许可证)

---

## 项目简介

`mosbsfol` 在 Linux 上复刻六个最典型的 macOS 文件系统痕迹，并附带一个可开关的
`autopoop` daemon，在可移动设备插入时、以及在本机固定磁盘上自动“拉屎”。

| 痕迹 | 说明 |
| --- | --- |
| `.DS_Store` | Finder 视图状态 |
| `._文件名` / AppleDouble | Resource Fork、FinderInfo 的侧车文件 |
| `__MACOSX/` | Finder 风格 ZIP 元数据 |
| `.plist` | XML / `bplist00` |
| `xattr` | quarantine、FinderInfo、ResourceFork、WhereFroms、Tags、Hidden |
| 卷根目录痕迹 | `.Spotlight-V100`、`.fseventsd`、`.Trashes`、`.TemporaryItems`、`.localized`、`.VolumeIcon.icns`、`Icon\r` |
| `autopoop` | daemon + udev trigger + 运行时开关 |

---

## 快速开始

### 构建与安装

```sh
cargo build --release
alias mosbsfol="$PWD/target/release/mosbsfol"

# 或安装到 ~/.cargo/bin
cargo install --path .
```

### 30 秒上手

```sh
# 查看全部命令
mosbsfol --help

# 手动给 U 盘挂载点来一套“被 Mac 插过”的痕迹
mosbsfol usb /mnt/usb -r --include-dirs --type-codes

# 打开自动拉屎开关
mosbsfol autopoop enable

# 先 dry-run 看看本机哪些磁盘会被拉
mosbsfol autopoop local --dry-run

# 确认没问题后，前台跑 daemon（Ctrl-C 停止）
mosbsfol autopoop run --interval 2

# 不需要时关掉
mosbsfol autopoop disable
```

---

## Feature 一览

默认启用全部 feature：

```toml
default = ["dsstore", "appledouble", "maczip", "plist", "xattr", "volumetrace", "autopoop"]
```

| Cargo feature | 命令 | 功能 |
| --- | --- | --- |
| `dsstore` | `dsstore` / `poop` | 生成、解析、清理 Finder 可读的 `.DS_Store` |
| `appledouble` | `usb` | AppleDouble v2 `._*` 侧车；自动携带 FinderInfo 与 Resource Fork |
| `maczip` | `maczip` / `zip` | 生成带 `__MACOSX/._*` 条目的 Finder 风格 ZIP |
| `plist` | `plist` | 读写 XML plist 与 `bplist00` 二进制 plist |
| `xattr` | `xattr` | `com.apple.*` xattr：quarantine、FinderInfo、ResourceFork、WhereFroms、Tags、Hidden |
| `volumetrace` | `trace` / `volumetrace` | 卷根目录痕迹：`.Spotlight-V100`、`.fseventsd`、`.Trashes` 等 |
| `autopoop` | `autopoop` / `daemon` | 可移动设备 + 本机磁盘自动拉屎，支持运行时开关 |

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

# 自动拉屎
cargo build --no-default-features --features autopoop
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

生成合法 stored-ZIP：原始文件 + `__MACOSX/<相对路径>/._<文件名>`。

```sh
mosbsfol maczip ~/Documents ~/Documents.zip
mosbsfol maczip ~/Documents --dry-run
```

### plist

```sh
mosbsfol plist write demo.plist name=mosbsfol answer=42 pi=3.14
mosbsfol plist write demo.xml name=mosbsfol --xml
mosbsfol plist read demo.plist
```

### xattr

Linux 没有 macOS 命名空间时会自动把 `com.apple.*` 映射为 `user.com.apple.*`。

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

## autopoop：自动拉屎

`autopoop` 是常驻 daemon；可选配 udev rule 即时触发，或用 systemd service 托管。

### 拉什么

| 目标 | 触发方式 | 内容 | 递归行为 |
| --- | --- | --- | --- |
| 可移动设备（U 盘 / SD 卡） | udev `trigger` 即时触发；daemon 轮询兜底 | `._*` + `.DS_Store` + 卷痕迹 | 默认递归 |
| 本机固定磁盘 | daemon 启动 / 新本地挂载 / `--local-rescan` 周期重拉 | `.DS_Store` + 卷痕迹，**不生成 `._*`** | 默认只拉挂载根目录；`--local-recursive` 递归 |

### 开关

开关是运行时状态文件：内容为 `enabled` 时开启，文件不存在即关闭（安全默认）。

- 普通用户默认：`$XDG_RUNTIME_DIR/mosbsfol/autopoop/state`
- udev / systemd 显式使用：`/run/mosbsfol/autopoop/state`
- 也可用 `--state FILE` 或 `MOSBSFOL_AUTOPOOP_STATE` 指定

```sh
mosbsfol autopoop enable    # 开
mosbsfol autopoop status    # 查
mosbsfol autopoop disable   # 关
```

### 手动与 daemon 命令

```sh
# 手动拉一次：可移动设备 + 本机固定磁盘（尊重开关，--force 可绕过）
mosbsfol autopoop once --dry-run
mosbsfol autopoop once /mnt/usb --force

# 只拉本机：默认所有本机磁盘挂载根；-r 递归，也可指定目录
mosbsfol autopoop local --dry-run
mosbsfol autopoop local ~/Documents -r --force

# 前台 daemon
# --interval        新挂载扫描间隔（默认 2 秒）
# --local-rescan    已挂载本机磁盘重拉间隔（默认 3600 秒）
# --no-local        只监听可移动设备，不碰本机
# --local-recursive 本机磁盘递归拉 .DS_Store
mosbsfol autopoop run --interval 2 --local-rescan 3600

# 模拟 udev 事件（MAJ:MIN，即 udev 的 %M:%m）
mosbsfol autopoop trigger 8:17 --force
# udev 事件也处理本机固定盘（默认跳过，交给 daemon）
mosbsfol autopoop trigger 8:17 --force --include-local
```

### 安装 udev rule（即时触发）

```sh
sudo cp packaging/udev/99-mosbsfol-autopoop.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

规则默认调用 `/usr/bin/mosbsfol`；二进制装在别处时请先修改规则里的绝对路径。
系统级开关必须和规则使用同一个状态文件：

```sh
sudo mosbsfol autopoop enable  --state /run/mosbsfol/autopoop/state
sudo mosbsfol autopoop disable --state /run/mosbsfol/autopoop/state
sudo mosbsfol autopoop status  --state /run/mosbsfol/autopoop/state
```

### 安装 systemd 服务（轮询后备）

```sh
sudo cp packaging/systemd/mosbsfol-autopoop.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now mosbsfol-autopoop
```

unit 默认使用 `/usr/bin/mosbsfol`，并按 3600 秒周期重拉本机磁盘；按需修改 `ExecStart`。

### 注意事项

- udev `add` 事件可能早于桌面环境自动挂载；此时 trigger 只会报告“还没有挂载”，daemon 轮询会在挂载完成后补拉。
- udev 与 systemd 同时运行无害：重复触发是幂等的，卷痕迹只创建一次。
- 本机自动拉屎范围大：请先用 `autopoop local --dry-run` / `once --dry-run` 确认清单，再真正 `enable`。
- 开关开启时，daemon 启动会先把当前已挂载的可移动设备和本机磁盘都拉一遍，然后持续监听。

---

## 已知限制

| 项目 | 说明 |
| --- | --- |
| `.DS_Store` | Apple 未公开格式；当前生成单叶节点 B-tree，排序为 HFS+ `TN1150` 的近似 |
| AppleDouble | Resource Fork 按原始字节写入，不解析内部结构 |
| `__MACOSX` | ZIP 使用 stored（无压缩）方式；结构正确但不是 Finder 逐字节复刻 |
| `.VolumeIcon.icns` / `Icon\r` | 只写入 8 字节合法 ICNS 头占位，不生成真实图标 |
| xattr | 需要文件系统支持 `user` xattr；FAT/exFAT/tmpfs 不支持时应改用 `usb` 的 AppleDouble 路径 |
| `.DS_Store` 文件名 | Linux 非法 UTF-8 文件名会 lossy 转为 U+FFFD |
| 大目录 | 单叶节点设计足够普通目录；接近 1 GiB 的极端目录需扩展为多节点 B-tree |
| `usb --clean` / `trace clean` | 会递归删除 `.Spotlight-V100`、`.Trashes` 等标记目录及其内容；重要数据请先 `--dry-run` |
| `maczip` | 目标 ZIP 已存在时会直接覆盖 |
| `autopoop` | 默认关闭；`enable` 后会对插入的可移动设备和本机固定磁盘写文件，请先 `once --dry-run` / `local --dry-run` 确认 |
| `autopoop` 本机 | 默认只拉挂载根目录 `.DS_Store` + 卷痕迹，不生成 `._*`；`-r` / `--local-recursive` 才递归本机目录树 |
| `autopoop` | 不会自动挂载设备；依赖 udisks / 桌面环境挂载。udev `add` 事件可能早于挂载，daemon 轮询会补拉 |
| `autopoop` | sysfs 不可用时按文件系统类型（vfat/exfat/ntfs/... 与 ext4/xfs/btrfs/...）猜测可移动/本机，极端环境下可能误判 |

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
