#!/usr/bin/env python3
"""Timestamped serial monitor that tolerates the USB-Serial/JTAG re-enumeration
flakiness of the ESP32-S3-PICO. Usage: serial-watch.py [seconds]"""
import sys, time, datetime

PORT = "/dev/cu.usbmodem2101"
BAUD = 115200
deadline = time.time() + float(sys.argv[1] if len(sys.argv) > 1 else 180)

import serial  # noqa: E402

def stamp():
    return datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]

buf = b""
while time.time() < deadline:
    try:
        with serial.Serial(PORT, BAUD, timeout=1) as ser:
            print(f"[{stamp()}] --- attached {PORT} ---", flush=True)
            while time.time() < deadline:
                chunk = ser.read(4096)
                if not chunk:
                    continue
                buf += chunk
                while b"\n" in buf:
                    line, buf = buf.split(b"\n", 1)
                    text = line.decode("utf-8", "replace").rstrip("\r")
                    if text.strip():
                        print(f"[{stamp()}] {text}", flush=True)
    except serial.SerialException as e:
        print(f"[{stamp()}] --- port gone ({e}); retrying ---", flush=True)
        time.sleep(1)
print(f"[{stamp()}] --- monitor done ---", flush=True)
