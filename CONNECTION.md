# ESP32 墨水屏设备 · 连接信息

> 检测时间：2026-09-02 · macOS (darwin 24.6.0, arm64) · esptool v5.4.0 (via `uvx esptool`)

## 设备识别结果

| 项目 | 信息 |
|------|------|
| 芯片 | **ESP32-S3** · ESP32-S3-PICO-1 (LGA56 封装) · rev v0.2 |
| Flash | **8 MB**，兆易创新 (GD)，封装内嵌 |
| PSRAM | **8 MB** (AP_3v3)，封装内嵌 |
| 无线 | Wi-Fi + BLE 5，双核 LX7 240 MHz + LP 核 |
| 晶振 | 40 MHz |
| USB | 芯片原生 USB-Serial/JTAG |
| MAC 地址 | `70:04:1d:d7:e7:c4`（USB 序列号同此值） |

> S3-PICO-1 是把 flash/PSRAM 封进芯片内的单芯片方案，板上没有独立模组 —— 说明这是一块定制板/成品板，不是常见开发板。

## 串口拓扑：板上有两个 USB 口

| 端口 | 后面是什么 | 状态 |
|------|-----------|------|
| `/dev/cu.usbmodem2020_12_222` | 外挂 **LDR2001**（深圳乐得瑞，VID `0x2D79` PID `0x0003`，免驱 CDC）→ UART0 | ⚠️ 芯片能枚举，但 **UART0 全静默**（多波特率 + 复位时序均无输出） |
| `/dev/cu.usbmodem2101` | ESP32-S3 **原生 USB-Serial/JTAG**（VID `0x303A`，序列号 = MAC） | ✅ **烧录/日志都用这个口** |

- VID `0x303A` = 乐鑫官方 VID；设备名 "USB JTAG_serial debug unit"。
- LDR2001 口以后可留作参考（固件若重定向日志到 UART0 才有用）。

## 已验证可用的命令

```bash
# 识别芯片
uvx esptool --port /dev/cu.usbmodem2101 chip-id

# 读 flash（读出现有固件，8MB）
uvx esptool --port /dev/cu.usbmodem2101 read-flash 0x0 0x800000 flash-dump.bin

# 烧录
uvx esptool --port /dev/cu.usbmodem2101 write-flash 0x0 firmware.bin
```

无需手动进下载模式：USB-Serial/JTAG 由 esptool 自动复位接管。

## 已知问题：端口会闪断

- 端口节点会周期性消失/重现（几秒量级），偶尔出现 8~10 秒的 "No serial data received" 窗口。
- 疑似原因（按可能性排序）：
  1. **墨水屏刷新瞬间电流冲击 → brownout 复位**（S3 + 屏驱动 + Wi-Fi 峰值对 USB 5V 供电压力大）
  2. 固件崩溃循环重启
  3. 线材/接触不良
- 复现/观察方法：看屏幕是否周期性自刷新、指示灯是否规律闪烁。

## 应对闪断的抢连脚本

端口一出现立刻连接（已在本次检测中验证有效）：

```bash
for i in $(seq 1 360); do
  PORT=$(ls /dev/cu.usbmodem* 2>/dev/null | grep -v 2020_12_222 | head -1)
  if [ -n "$PORT" ]; then
    uvx esptool --port "$PORT" chip-id && break
  fi
  sleep 0.25
done
```

## 环境备忘

- 本机无 esptool/pyserial 常驻安装，统一走 `uvx esptool`（uv 缓存已建好，秒起）。
- pyserial 临时依赖：`uv run --with pyserial python <script>`。
- esptool 每次操作完会 hard-reset 芯片，导致 USB 重新枚举（端口闪断的一部分是正常现象，注意区分）。

## 待排查

- [x] ~~墨水屏驱动型号与引脚定义~~ —— 已确认是 Waveshare ESP32-S3-ePaper-1.54G 成品板（1.54G 四色 200×200，引脚与官方一致）。**关键坑：GPIO6 = EPD3V3_EN 面板供电开关，低电平有效，悬空则面板不上电**（TCON 靠 SPI 寄生供电仍会应答、BUSY 照常翻转，但墨滴不动、无任何闪烁）。固件必须将 GPIO6 输出低电平。
- [ ] 周期性重启的根因（brownout 还是固件崩溃）
- [ ] LDR2001 是否真的接到 UART0（焊接确认）
