# esp32-epaper

ESP32-S3 驱动 1.54" 四色墨水屏（黑/白/红/黄，200×200，2bpp）的 `no_std` Rust 固件。

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

## 排障记录：屏幕「有日志、无画面」

**症状**：固件日志完全正常（`refresh completed=true`，BUSY 被拉低约 24 秒，与真实全刷时长吻合），但屏幕没有任何闪烁、画面不变。

**根因**：GPIO6（EPD3V3_EN）悬空未驱动 → 面板 3.3V 供电未开。TCON 逻辑靠 SPI 信号线的**寄生供电**维持工作，所以 SPI 应答正常、BUSY 照常翻转、波形状态机空跑 24 秒，但驱动墨滴的高压部分没电，画面纹丝不动。

**定位手法**：把固件改成全黑/全白每 ~45 秒交替刷新。若数据通路坏则闪烁但内容不变，若面板没电则连闪烁都没有——实测完全无闪烁，锁定供电问题。对照 [Waveshare 官方文档](https://docs.waveshare.com/ESP32-S3-ePaper-1.54G)确认 GPIO6 = EPD3V3_EN 后修复。

**修复**：`firmware/src/main.rs` 在 `esp_hal::init` 后立即将 GPIO6 输出低电平（高边 P-MOSFET，低 = 开）。修复后黑白交替可见，Hello World 正常显示。

> 教训：这块板的参考驱动（`reference/epaper_port.c`，来自小智固件仓库）不碰 GPIO6，不能据此假设「供电默认开启」。

## 固件行为

`epaper-hello`：开机延时 2s → 初始化面板 → 绘制并全刷一次画面（红框 + "Hello World!" 等）→ 面板进 deep sleep → 主循环空转。画面静态保持属预期（墨水屏断电也能保持）。

寄存器序列逐条移植自 Waveshare 参考驱动 `reference/EPD_1in54g.cpp`，面板异常时以它为准对齐。

## 文件结构

```
firmware/           Rust 固件（esp-hal 1.2，no_std）
  src/main.rs       应用入口 + GPIO6 供电修复
  src/epd.rs        1.54G 驱动（2bpp 帧缓冲 + embedded-graphics DrawTarget）
reference/          Waveshare / 小智固件参考代码（C/C++）
CONNECTION.md       端口拓扑、esptool 命令、已知问题
epaper-hello-app.bin  当前固件的应用镜像（elf2image 产物）
full-flash.bin      原厂固件 8MB 整盘备份（2026-09-02 dump）
```
