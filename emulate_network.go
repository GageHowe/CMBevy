package main

import (
	"flag"
	"log"
	"math/rand"
	"net"
	"time"
)

type linkConfig struct {
	lossProb   float64
	minDelay   time.Duration
	maxDelay   time.Duration
	bufferSize int
}

func main() {
	addrA := flag.String("a", ":5000", "UDP listen address for side A (e.g. :5000)")
	addrB := flag.String("b", ":6000", "UDP listen address for side B (e.g. :6000)")
	loss := flag.Float64("loss", 0.1, "packet loss probability (0.0 - 1.0)")
	minDelayMs := flag.Int("mindelay", 20, "minimum one-way delay in ms")
	maxDelayMs := flag.Int("maxdelay", 200, "maximum one-way delay in ms")
	bufSize := flag.Int("buf", 65535, "UDP buffer size")
	flag.Parse()

	if *maxDelayMs < *minDelayMs {
		log.Fatalf("maxdelay must be >= mindelay")
	}

	cfg := linkConfig{
		lossProb:   *loss,
		minDelay:   time.Duration(*minDelayMs) * time.Millisecond,
		maxDelay:   time.Duration(*maxDelayMs) * time.Millisecond,
		bufferSize: *bufSize,
	}

	rand.Seed(time.Now().UnixNano())

	connA, err := net.ListenPacket("udp", *addrA)
	if err != nil {
		log.Fatalf("listen A (%s): %v", *addrA, err)
	}
	defer connA.Close()

	connB, err := net.ListenPacket("udp", *addrB)
	if err != nil {
		log.Fatalf("listen B (%s): %v", *addrB, err)
	}
	defer connB.Close()

	log.Printf("Listening A=%s, B=%s", *addrA, *addrB)
	log.Printf("Loss=%.2f, delay=[%s, %s]", cfg.lossProb, cfg.minDelay, cfg.maxDelay)

	var lastA, lastB net.Addr

	// A -> B reader
	go func() {
		buf := make([]byte, cfg.bufferSize)
		for {
			n, addr, err := connA.ReadFrom(buf)
			if err != nil {
				log.Printf("read A: %v", err)
				continue
			}
			// capture data for this packet
			data := make([]byte, n)
			copy(data, buf[:n])

			lastA = addr
			if lastB == nil {
				continue
			}

			// one goroutine per packet
			go forwardWithImpairments(connB, lastB, data, cfg)
		}
	}()

	// B -> A reader
	go func() {
		buf := make([]byte, cfg.bufferSize)
		for {
			n, addr, err := connB.ReadFrom(buf)
			if err != nil {
				log.Printf("read B: %v", err)
				continue
			}
			data := make([]byte, n)
			copy(data, buf[:n])

			lastB = addr
			if lastA == nil {
				continue
			}

			// one goroutine per packet
			go forwardWithImpairments(connA, lastA, data, cfg)
		}
	}()

	select {}
}

func forwardWithImpairments(conn net.PacketConn, dst net.Addr, data []byte, cfg linkConfig) {
	// per-packet loss
	if rand.Float64() < cfg.lossProb {
		return
	}

	// per-packet random delay in [minDelay, maxDelay]
	jitterRange := cfg.maxDelay - cfg.minDelay
	var delay time.Duration
	if jitterRange <= 0 {
		delay = cfg.minDelay
	} else {
		delay = cfg.minDelay + time.Duration(rand.Int63n(int64(jitterRange)))
	}
	if delay > 0 {
		time.Sleep(delay)
	}

	if _, err := conn.WriteTo(data, dst); err != nil {
		log.Printf("write to %v: %v", dst, err)
	}
}
