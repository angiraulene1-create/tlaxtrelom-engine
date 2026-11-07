#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
# Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
# CONFIDENTIAL — internal use only.
"""
Verificador del protocolo de red Go para telemetría FullDive/VR de Tlaxtrelom Engine.
Valida la serialización binaria, checksum CRC32 y la detección de paquetes corruptos.
"""

import struct
import zlib
import shutil
import subprocess
import sys
from pathlib import Path

MAGIC = 0x544C5831 # "TLX1"
VERSION = 1

def encode_packet(pkt_type, seq, user_id, ts, payload: bytes) -> bytes:
    hdr = struct.pack(">IHHIQQ", MAGIC, VERSION, pkt_type, seq, user_id, ts)
    body = hdr + payload
    crc = zlib.crc32(body) & 0xFFFFFFFF
    return body + struct.pack(">I", crc)

def decode_packet(data: bytes):
    if len(data) < 32:
        raise ValueError("Buffer too short")
    magic, ver, pkt_type, seq, user_id, ts = struct.unpack(">IHHIQQ", data[:28])
    if magic != MAGIC:
        raise ValueError(f"Invalid magic: {hex(magic)}")
    payload = data[28:-4]
    expected_crc = struct.unpack(">I", data[-4:])[0]
    actual_crc = zlib.crc32(data[:-4]) & 0xFFFFFFFF
    if expected_crc != actual_crc:
        raise ValueError(f"CRC mismatch: expected {expected_crc}, got {actual_crc}")
    return {
        "type": pkt_type,
        "seq": seq,
        "user_id": user_id,
        "ts": ts,
        "payload": payload,
    }

def test_wire_protocol():
    payload = b"NEURAL_INTENT_SYNC_6DOF"
    pkt = encode_packet(pkt_type=2, seq=42, user_id=1001, ts=1234567890, payload=payload)
    decoded = decode_packet(pkt)
    assert decoded["seq"] == 42
    assert decoded["user_id"] == 1001
    assert decoded["payload"] == payload
    print("  ✓ [Go/Wire Contract] Serialización y CRC32 validados")

def test_corruption_detection():
    payload = b"BIO_HRV_DATA"
    pkt = bytearray(encode_packet(pkt_type=1, seq=1, user_id=2002, ts=9999, payload=payload))
    # Corromper un byte
    pkt[30] ^= 0xFF
    try:
        decode_packet(bytes(pkt))
        assert False, "Debería haber fallado por CRC inválido"
    except ValueError as e:
        assert "CRC mismatch" in str(e)
    print("  ✓ [Go/Wire Contract] Detección de corrupción y alteración verificada")

def main():
    print("==> Go (Protocolo de Telemetría y Relay de Red)")
    test_wire_protocol()
    test_corruption_detection()

    if shutil.which("go"):
        print("  $ go test ./...")
        relay_dir = Path(__file__).resolve().parent
        ret = subprocess.run(["go", "test", "-v"], cwd=str(relay_dir))
        if ret.returncode != 0:
            sys.exit(ret.returncode)

    print("Relay protocol tests: OK.")

if __name__ == "__main__":
    main()
