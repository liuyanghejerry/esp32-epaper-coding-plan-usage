# esp32-epaper

ESP32-S3 驱动 1.54" 四色墨水屏（黑/白/红/黄，200×200，2bpp）的 `no_std` Rust 固件。

当前固件：**Kimi Code plan usage 监视器** —— 连 Wi-Fi 每 5 分钟查询一次 coding plan 用量，数值有变化才刷新墨水屏（护屏）。

## 硬件

成品板：**Waveshare ESP32-S3-ePaper-1.54G**（ESP32-S3-PICO-1-N8R8，8MB Flash + 8MB PSRAM，板载 ES8311 音频 codec、SHTC3、TF 卡槽、PCF85063 RTC、锂电池充放电管理）。

墨水屏接口（SPI2）：

| 功能 | GPIO | 备注 |
|------|------|------|
| EPD3V3_EN | **GPIO6** | **面板 3.3V 供电开关，低电平有效 —— 必须主动拉低！** |
| EPD_BUSY | GPIO8 | 空闲高电平，刷新期间拉低（全刷约 20~24s） |
| EPD_RST | GPIO9 | |
| EPD_D/C | GPIO10 | |
| EPD_CS | GPIO11 | |
| EPD_SCLK | GPIO12 | |
| EPD_SDI (MOSI) | GPIO13 | |

其他：BOOT 键 = GPIO0；电池供电锁存 = GPIO17（BAT_Control），电池键 = GPIO18（BAT_KEY）。串口/烧录端口见 [CONNECTION.md](CONNECTION.md)。

## 构建

需要 espup 安装的 `esp` 工具链（Xtensa 目标），编译前必须 source 环境：

```bash
cd firmware
. ~/export-esp.sh            # 提供 xtensa-esp32s3-elf-gcc 链接器等
cargo build --release
```

## 烧录

本机用 `uvx esptool`（无常驻 esptool 安装）。分三步：

```bash
# 1. ELF -> 应用镜像（flash 参数必须 dio / 80MHz / 8MB，与厂固件一致）
uvx esptool --chip esp32s3 elf2image \
  --flash-mode dio --flash-freq 80m --flash-size 8MB \
  firmware/target/xtensa-esp32s3-none-elf/release/epaper-hello \
  -o epaper-hello-app.bin

# 2. 写入 0x10000（factory 应用分区，4MB；bootloader 和分区表不动）
uvx esptool --port /dev/cu.usbmodem2101 --chip esp32s3 \
  write-flash 0x10000 epaper-hello-app.bin
```

注意：

- 烧录/日志都走 `/dev/cu.usbmodem2101`（ESP32-S3 原生 USB-Serial/JTAG），不要用另一个 `usbmodem2020_12_222` 口（外挂 LDR2001，UART0 静默）。
- 端口存在周期性闪断问题，烧录建议套抢连循环（见 CONNECTION.md「应对闪断的抢连脚本」）。
- `write-flash` 自带 hash 校验，结束后自动硬复位运行新固件。

## 固件行为（usage 监视器）

启动流程：开 GPIO6 面板供电 → 起 embassy 运行时 → 连 Wi-Fi（DHCP）→ 查询 usage → 数据到手即渲染刷屏 → 之后每 5 分钟查一次，**数值不变不刷屏**（每次全刷约 20s，墨水屏有刷新寿命）。连续 3 次查询失败且尚无成功数据时画红色错误页。主循环每 30s 打一条心跳日志（含最近一次用量），方便随时挂串口确认设备活着。

电源管理：电池放电回路被电源键（GPIO18，按下为低）门控——按住才通电，所以固件在启动最初就把 **GPIO17（BAT_Control）拉高锁存**（参照官方 08_BATT_PWR demo），松开按键后电池继续供电；USB 5V 直达 DC-DC，不经此锁存。**长按电源键 ≥2s 触发软关机**：先刷一页白底 "POWER OFF" 画面（墨水屏断电后永久保留图像，不刷会让关机后的板子看起来像"冻结的旧数据"），再拉低 GPIO17 释放锁存、切掉面板 3V3——电池供电下板子即刻断电，USB 供电下则驻留待机（相当于"结束本次会话"）。按键只在 30s 心跳空窗内以 50ms 轮询，若长按恰好落在刷屏/联网期间会漏检，松开重按即可。

usage API（与 Kimi Code CLI 的 `/usage` 相同）：

```text
GET https://api.kimi.com/coding/v1/usages
Authorization: Bearer <api-key>     Accept: application/json
```

响应里的配额数字是**字符串**；固件提取 `usage`（每周配额 + `resetTime`，显示为 UTC+8）和 `limits[0]`（5 小时滚动窗），`boosterWallet` 等忽略。

画面布局（数据驱动，见下文「界面布局」）：黑底标题栏 "Kimi Code Usage"，周配额数值 + 进度条（≥80% 变红）、每周 reset 时间，5h 滚动窗用量 + 进度条、reset 时间。界面上的所有位置参数在 `firmware/layout.json` 里。

### 配置（Wi-Fi / API key）

凭证在仓库根目录的 `.env`（**已 gitignore**），编译时由 `build.rs` 注入二进制：

```bash
cp .env.example .env
# 填入 WIFI_SSID / WIFI_PASSWORD（仅 2.4GHz，S3 不支持 5G）
# 填入 KIMI_TOKEN：长期 API key（sk-kimi-...，Kimi Code Console 签发，
# 本机 JS 版 coding-usage-bar 配置 ~/.coding-usage-bar/config.json 里也有）
```

注意：CLI 的 OAuth token（`~/.kimi-code/credentials/kimi-code.json`）也能用，但**15 分钟就过期**，只适合临时验证；refresh_token 流程不能搬到设备端（刷新会轮换，会把本机 CLI 踢下线）。

### 界面布局（数据 / 展示分离）

所有界面位置参数在 `firmware/layout.json`，不在代码里。`build.rs` 在编译期解析它并生成 `OUT_DIR/layout.rs` 常量，`src/render.rs` 通过 `include!` 引用——改布局只需改 JSON 再重新编译。

微调用可视化编辑器 `tools/layout-editor.html`（单文件，浏览器直接打开）：按 200×200 面板 1:1 建模预览（含顶部物理遮挡区提示），可拖拽或选中后方向键微调（Shift=10px），支持整组/单元素两种移动模式；「导出 JSON」生成的就是 `layout.json` 的内容（复制或下载），「导入 JSON」可把已保存的布局重新载入继续编辑。布局也会自动存在浏览器 localStorage。

```jsonc
{
  "band_h": 28,                 // 顶部黑条高度
  "title": { "x": 10, "y": 13 },// 标题 "Kimi Code Usage"（y 限制在黑条内）
  "week": { "y": 47 },          // Week 行（label 左对齐 + 数值右对齐）
  "bar1": { "y": 62 },          // 周进度条
  "reset1": { "x": 10, "y": 86 },// 每周 reset 时间
  "win": { "y": 121 },          // 5h 行
  "bar2": { "y": 136 },         // 滚动窗进度条
  "reset2": { "x": 10, "y": 160 },
  "text_left": 10,              // label 左缘 x
  "text_right": 190,            // 数值右缘 x
  "bar": { "x": 10, "w": 180, "h": 14 }  // 两条进度条共有样式
}
```

`build.rs` 会校验范围（与编辑器一致），越界直接编译失败并报原因。

## 排障记录：屏幕「有日志、无画面」

**症状**：固件日志完全正常（`refresh completed=true`，BUSY 被拉低约 24 秒，与真实全刷时长吻合），但屏幕没有任何闪烁、画面不变。

**根因**：GPIO6（EPD3V3_EN）悬空未驱动 → 面板 3.3V 供电未开。TCON 逻辑靠 SPI 信号线的**寄生供电**维持工作，所以 SPI 应答正常、BUSY 照常翻转、波形状态机空跑 24 秒，但驱动墨滴的高压部分没电，画面纹丝不动。

**定位手法**：把固件改成全黑/全白每 ~45 秒交替刷新。若数据通路坏则闪烁但内容不变，若面板没电则连闪烁都没有——实测完全无闪烁，锁定供电问题。对照 [Waveshare 官方文档](https://docs.waveshare.com/ESP32-S3-ePaper-1.54G)确认 GPIO6 = EPD3V3_EN 后修复。

**修复**：`firmware/src/main.rs` 在 `esp_hal::init` 后立即将 GPIO6 输出低电平（高边 P-MOSFET，低 = 开）。修复后黑白交替可见，Hello World 正常显示。

> 教训：这块板的参考驱动（`reference/epaper_port.c`，来自小智固件仓库）不碰 GPIO6，不能据此假设「供电默认开启」。

## 排障记录：esp-hal 1.1 的 `SpiBus::write` 不 flush（联网版固件「日志成功、屏幕不变」）

**症状**：Wi-Fi、HTTPS 查询、渲染全部正常（日志 `refresh completed=true`），但屏幕连闪烁都没有，画面不变。

**根因**：esp-hal **1.1** 的 `SpiBus::write` trait 实现把 FIFO 装满、启动传输后**立即返回**（源码注释原话："The trait impl does not flush after"）。驱动里 `write()` 之后马上拉高 CS，每个命令/数据包的最后一个块被截断，面板根本收不到命令，BUSY 一直停在空闲高电平，于是 `wait_busy` 也「正常」通过——全链路假成功。旧 Hello World 固件用的是 esp-hal **1.2**（其 trait 实现委托给会等传输完成的 inherent `write`），所以当时没事；联网版因 esp-radio 兼容性降到 1.1 才踩中。

**修复**：`firmware/src/epd.rs` 的 `cmd()`/`data()` 在拉高 CS 前显式 `spi.flush()`。

## 排障记录：esp-hal 1.1 的 `SpiConfig::with_frequency` 杀死 SCLK

**症状**：上一条修完后面板依然「假成功、无画面」。加埋点后发现决定性证据：`epd.init()` 的 power-on 等待和 `display()` 的刷新等待都**瞬间返回**（10ms 而非正常的 70ms/17-20s），BUSY 全程停在空闲高电平——面板根本没收到任何命令。

**二分过程**：重刷 git 里的旧 Hello World 固件 → 屏幕正常刷新 → 硬件/接线无罪，锁定新固件。随后逐项回退新固件与旧固件的差异：CPU 240MHz + 默认 SPI → 正常；CPU 240MHz + 显式 4MHz → 失聪。实锤：**esp-hal 1.1.2 上只要给 SPI 传显式频率（4MHz、20MHz 都试过），SCLK 就完全不翻转**。默认配置（1MHz）则正常。寄存器级的具体机制未深挖（`recalculate()` 的分频数学看起来是对的，嫌疑在寄存器应用路径）。

**修复**：`SpiConfig::default()`，不调用 `with_frequency`。10KB 帧缓冲在 1MHz 下传输仅 ~80ms，刷新本身要 20s，速度毫无意义。

> 关联线索：旧 Hello World 固件能工作还有一个原因是它用 esp-hal **1.2**——1.2 的 `SpiBus::write` 委托给会等传输完成的 inherent 方法，自带 flush 语义；1.1 两者皆有坑（trait write 不 flush + with_frequency 死时钟）。

## 排障记录：TLS `HandshakeFailure`

**症状**：`GET https://api.kimi.com/...` 在 TLS 握手阶段被服务器拒绝（`HandshakeAborted(Fatal, HandshakeFailure)`）。

**根因**：api.kimi.com 前端是火山引擎 WAF，证书链是 **RSA-only**；embedded-tls 默认只在 ClientHello 里宣告 ECDSA/Ed25519 签名算法，服务器无共同算法直接中止。

**修复**：`reqwless` 开 `rsa` feature（→ embedded-tls 宣告 RSA 签名方案并能验证 RSA 签名）。同时记得 `der = "=0.8.0"` + `heapless` feature 的 pin（embedded-tls 的 `der_certificate.rs` 需要，但它自己 manifest 没开）。

### 版本组合（踩坑后的可行解，别乱动）

esp-hal **1.1** + esp-rtos 0.3 + esp-alloc 0.10 + esp-bootloader-esp-idf **0.5** + embassy-{executor 0.10, net 0.9, time 0.5} + reqwless 0.14（内置 embedded-tls 0.18）+ heapless **0.8**（serde-json-core 0.6 只认 0.8）。已发布的 esp-radio 1.0.0-beta.0 只兼容 esp-hal ~1.1；esp-bootloader-esp-idf 0.6 会把 esp-hal 1.2.0-rc 拉进来造成冲突。

## 文件结构

```
firmware/           Rust 固件（esp-hal 1.1，no_std，embassy 异步）
  src/main.rs       应用入口：Wi-Fi/网络任务 + 5 分钟轮询主循环 + GPIO6 供电
  src/epd.rs        1.54G 驱动（2bpp 帧缓冲 + embedded-graphics DrawTarget）
  src/usage.rs      usage API 客户端（reqwless HTTPS + serde-json-core 解析）
  src/render.rs     usage 画面 / 错误页布局（坐标编译期来自 layout.json）
  layout.json       界面布局数据（可视化编辑器 tools/layout-editor.html 导出）
  build.rs          链接参数 + 从根目录 .env 注入凭证 + 解析 layout.json 生成布局常量
reference/          Waveshare / 小智固件参考代码（C/C++）
tools/layout-editor.html  200×200 界面布局可视化编辑器（单文件，导出/导入 layout.json）
tools/serial-watch.py  带时间戳的串口监听（容忍端口闪断重附）
CONNECTION.md       端口拓扑、esptool 命令、已知问题
.env.example        凭证模板（复制为 .env，gitignored，编译期注入二进制）
epaper-hello-app.bin  当前固件的应用镜像（elf2image 产物，不入库）
full-flash.bin      原厂固件 8MB 整盘备份（2026-09-02 dump，已入库）
```
