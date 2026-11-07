// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

package main

import (
	"encoding/binary"
	"errors"
	"hash/crc32"
)

const (
	PacketMagic   uint32 = 0x544C5831 // "TLX1"
	PacketVersion uint16 = 1

	TypeHeartbeat     uint16 = 0
	TypeVrHeadPose    uint16 = 1
	TypeFullDiveState uint16 = 2
	TypeBiometricPanic uint16 = 3
)

// Header del paquete de red de telemetría Tlaxtrelom (24 bytes).
type PacketHeader struct {
	Magic     uint32
	Version   uint16
	Type      uint16
	Sequence  uint32
	UserID    uint64
	Timestamp uint64
}

type NetworkPacket struct {
	Header  PacketHeader
	Payload []byte
	CRC32   uint32
}

var (
	ErrInvalidMagic = errors.New("tlaxtrelom: invalid packet magic")
	ErrCorruptCRC   = errors.New("tlaxtrelom: packet crc32 mismatch")
	ErrTooShort     = errors.New("tlaxtrelom: packet buffer too short")
)

// EncodePacket serializa un paquete a formato binario de red.
func EncodePacket(hdr PacketHeader, payload []byte) []byte {
	buf := make([]byte, 28+len(payload)+4)
	binary.BigEndian.PutUint32(buf[0:4], PacketMagic)
	binary.BigEndian.PutUint16(buf[4:6], PacketVersion)
	binary.BigEndian.PutUint16(buf[6:8], hdr.Type)
	binary.BigEndian.PutUint32(buf[8:12], hdr.Sequence)
	binary.BigEndian.PutUint64(buf[12:20], hdr.UserID)
	binary.BigEndian.PutUint64(buf[20:28], hdr.Timestamp)

	copy(buf[28:28+len(payload)], payload)

	// CRC32 sobre todo el encabezado y payload
	checksum := crc32.ChecksumIEEE(buf[:28+len(payload)])
	binary.BigEndian.PutUint32(buf[28+len(payload):], checksum)

	return buf
}

// DecodePacket deserializa y valida la integridad del paquete.
func DecodePacket(data []byte) (*NetworkPacket, error) {
	if len(data) < 32 {
		return nil, ErrTooShort
	}

	magic := binary.BigEndian.Uint32(data[0:4])
	if magic != PacketMagic {
		return nil, ErrInvalidMagic
	}

	totalPayloadLen := len(data) - 32
	payloadEnd := 28 + totalPayloadLen
	expectedCrc := binary.BigEndian.Uint32(data[payloadEnd:])

	actualCrc := crc32.ChecksumIEEE(data[:payloadEnd])
	if expectedCrc != actualCrc {
		return nil, ErrCorruptCRC
	}

	pkt := &NetworkPacket{
		Header: PacketHeader{
			Magic:     magic,
			Version:   binary.BigEndian.Uint16(data[4:6]),
			Type:      binary.BigEndian.Uint16(data[6:8]),
			Sequence:  binary.BigEndian.Uint32(data[8:12]),
			UserID:    binary.BigEndian.Uint64(data[12:20]),
			Timestamp: binary.BigEndian.Uint64(data[20:28]),
		},
		Payload: make([]byte, totalPayloadLen),
		CRC32:   expectedCrc,
	}
	copy(pkt.Payload, data[28:payloadEnd])
	return pkt, nil
}
