// SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
// Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
// CONFIDENTIAL — internal use only.

package main

import (
	"fmt"
	"net"
	"sync"
	"time"
)

type Session struct {
	UserID       uint64
	LastAddr     *net.UDPAddr
	LastSequence uint32
	LastSeen     time.Time
	InPanic      bool
}

type RelayServer struct {
	mu          sync.RWMutex
	sessions    map[uint64]*Session
	conn        *net.UDPConn
	stopChan    chan struct{}
	panicCount  uint64
	packetCount uint64
}

func NewRelayServer(port int) (*RelayServer, error) {
	addr := &net.UDPAddr{Port: port}
	conn, err := net.ListenUDP("udp", addr)
	if err != nil {
		return nil, err
	}
	return &RelayServer{
		sessions: make(map[uint64]*Session),
		conn:     conn,
		stopChan: make(chan struct{}),
	}, nil
}

func (s *RelayServer) ProcessPacket(data []byte, src *net.UDPAddr) ([]byte, error) {
	pkt, err := DecodePacket(data)
	if err != nil {
		return nil, err
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	s.packetCount++

	sess, exists := s.sessions[pkt.Header.UserID]
	if !exists {
		sess = &Session{
			UserID:   pkt.Header.UserID,
			LastAddr: src,
		}
		s.sessions[pkt.Header.UserID] = sess
	}
	sess.LastAddr = src
	sess.LastSequence = pkt.Header.Sequence
	sess.LastSeen = time.Now()

	if pkt.Header.Type == TypeBiometricPanic {
		sess.InPanic = true
		s.panicCount++
		// Broadcast inmediato de parada a todos los pares
		return s.broadcastEmergency(sess.UserID), nil
	}

	return nil, nil
}

func (s *RelayServer) broadcastEmergency(userId uint64) []byte {
	hdr := PacketHeader{
		Type:      TypeBiometricPanic,
		Sequence:  0,
		UserID:    userId,
		Timestamp: uint64(time.Now().UnixNano()),
	}
	return EncodePacket(hdr, []byte("EMERGENCY_ESTOP_TRIGGERED"))
}

func (s *RelayServer) PruneStale(timeout time.Duration) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := time.Now()
	pruned := 0
	for id, sess := range s.sessions {
		if now.Sub(sess.LastSeen) > timeout {
			delete(s.sessions, id)
			pruned++
		}
	}
	return pruned
}

func main() {
	fmt.Println("Tlaxtrelom Telemetry & FullDive Relay daemon (Go)")
}
