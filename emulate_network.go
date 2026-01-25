package main

import (
	"flag"
	"fmt"
	"log"
	"math/rand"
	"net"
	"sync"
	"time"
)

type linkConfig struct {
	lossProb float64
	minDelay time.Duration
	maxDelay time.Duration
	bufSize  int
}

func main() {
	clientPort := flag.Int("client", 42069, "port to listen for client connections")
	serverPort := flag.Int("server", 42070, "port where server is listening")
	loss := flag.Float64("loss", 0.1, "packet loss probability (0.0 - 1.0)")
	minDelayMs := flag.Int("mindelay", 20, "minimum one-way delay in ms")
	maxDelayMs := flag.Int("maxdelay", 200, "maximum one-way delay in ms")
	bufSize := flag.Int("buf", 65535, "UDP buffer size")
	flag.Parse()

	if *maxDelayMs < *minDelayMs {
		log.Fatalf("maxdelay must be >= mindelay")
	}

	cfg := linkConfig{
		lossProb: *loss,
		minDelay: time.Duration(*minDelayMs) * time.Millisecond,
		maxDelay: time.Duration(*maxDelayMs) * time.Millisecond,
		bufSize:  *bufSize,
	}

	rand.Seed(time.Now().UnixNano())

	clientSideAddr, err := net.ResolveUDPAddr("udp", fmt.Sprintf("127.0.0.1:%d", *clientPort))
	if err != nil {
		log.Fatal(err)
	}
	clientSide, err := net.ListenUDP("udp", clientSideAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer clientSide.Close()

	serverAddr, err := net.ResolveUDPAddr("udp", fmt.Sprintf("127.0.0.1:%d", *serverPort))
	if err != nil {
		log.Fatal(err)
	}

	log.Printf("emulating network on port %d <-> %d\n", *clientPort, *serverPort)

	var clientAddr *net.UDPAddr
	var mu sync.Mutex

	buf := make([]byte, cfg.bufSize)
	for {
		n, addr, err := clientSide.ReadFromUDP(buf)
		if err != nil {
			log.Println("read error:", err)
			continue
		}
		isFromServer := addr.Port == serverAddr.Port

		if isFromServer {
			// server to client
			mu.Lock()
			dst := clientAddr
			mu.Unlock()

			if dst == nil {
				continue
			}

			// packet loss
			if rand.Float64() < cfg.lossProb {
				continue
			}

			packet := make([]byte, n)
			copy(packet, buf[:n])
			go sendDelayed(clientSide, packet, dst, &cfg)
		} else {
			// client to server
			mu.Lock()
			clientAddr = addr
			mu.Unlock()

			// packet loss
			if rand.Float64() < cfg.lossProb {
				continue
			}

			packet := make([]byte, n)
			copy(packet, buf[:n])
			go sendDelayed(clientSide, packet, serverAddr, &cfg)
		}
	}
}

func sendDelayed(conn *net.UDPConn, data []byte, dst *net.UDPAddr, cfg *linkConfig) {
	delay := cfg.minDelay
	if jitter := cfg.maxDelay - cfg.minDelay; jitter > 0 {
		delay += time.Duration(rand.Int63n(int64(jitter)))
	}

	time.Sleep(delay)

	if _, err := conn.WriteToUDP(data, dst); err != nil {
		log.Println("write error:", err)
	}
}
