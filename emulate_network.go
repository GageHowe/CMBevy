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

	listenAddr, err := net.ResolveUDPAddr("udp", fmt.Sprintf("127.0.0.1:%d", *clientPort))
	if err != nil {
		log.Fatal(err)
	}
	listener, err := net.ListenUDP("udp", listenAddr)
	if err != nil {
		log.Fatal(err)
	}
	defer listener.Close()

	serverAddr, err := net.ResolveUDPAddr("udp", fmt.Sprintf("127.0.0.1:%d", *serverPort))
	if err != nil {
		log.Fatal(err)
	}

	log.Printf("emulating network: clients on :%d <-> server on :%d (loss=%.0f%%, delay=%d-%dms)\n",
		*clientPort, *serverPort, cfg.lossProb*100, *minDelayMs, *maxDelayMs)

	// Map from client address string to their dedicated server connection.
	var clients sync.Map // string → *net.UDPConn

	buf := make([]byte, cfg.bufSize)
	for {
		n, clientAddr, err := listener.ReadFromUDP(buf)
		if err != nil {
			log.Println("read error:", err)
			continue
		}

		key := clientAddr.String()
		val, exists := clients.Load(key)
		if !exists {
			// New client — open a dedicated connection to the server.
			conn, err := net.DialUDP("udp", nil, serverAddr)
			if err != nil {
				log.Println("dial error:", err)
				continue
			}
			actual, loaded := clients.LoadOrStore(key, conn)
			if loaded {
				// Lost the race with another goroutine.
				conn.Close()
				val = actual
			} else {
				val = conn
				log.Printf("new client: %s", key)
				go forwardToClient(conn, listener, clientAddr, &cfg)
			}
		}

		if rand.Float64() < cfg.lossProb {
			continue
		}
		packet := make([]byte, n)
		copy(packet, buf[:n])
		serverConn := val.(*net.UDPConn)
		go sendDelayedConnected(serverConn, packet, &cfg)
	}
}

// forwardToClient reads server replies from conn and writes them back to the client.
func forwardToClient(conn *net.UDPConn, listener *net.UDPConn, clientAddr *net.UDPAddr, cfg *linkConfig) {
	buf := make([]byte, cfg.bufSize)
	for {
		n, err := conn.Read(buf)
		if err != nil {
			log.Printf("server conn closed for %s: %v", clientAddr, err)
			return
		}
		if rand.Float64() < cfg.lossProb {
			continue
		}
		packet := make([]byte, n)
		copy(packet, buf[:n])
		go sendDelayedTo(listener, packet, clientAddr, cfg)
	}
}

// sendDelayedConnected delays then writes on a DialUDP connection (no dst needed).
func sendDelayedConnected(conn *net.UDPConn, data []byte, cfg *linkConfig) {
	time.Sleep(jitter(cfg))
	if _, err := conn.Write(data); err != nil {
		log.Println("write error:", err)
	}
}

// sendDelayedTo delays then writes to dst on an unconnected listener socket.
func sendDelayedTo(conn *net.UDPConn, data []byte, dst *net.UDPAddr, cfg *linkConfig) {
	time.Sleep(jitter(cfg))
	if _, err := conn.WriteToUDP(data, dst); err != nil {
		log.Println("write error:", err)
	}
}

func jitter(cfg *linkConfig) time.Duration {
	d := cfg.minDelay
	if j := cfg.maxDelay - cfg.minDelay; j > 0 {
		d += time.Duration(rand.Int63n(int64(j)))
	}
	return d
}
