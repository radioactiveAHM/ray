# Ray

> **Ray** is an asynchronous VLESS server protocol implemented in Rust.  
> _Note: This project is intended as a personal portfolio piece and is not actively maintained. Pull requests will not be accepted._

## Transports

- [x] HTTP
- [x] HTTP Upgrade
- [x] WS (WebSocket)
- [x] XHTTP H2 stream-one + stream-up
- [ ] XHTTP H2 packet-up

## Vless Request Commands

- [x] TCP
- [x] UDP
- [x] XUDP

## Configuration File `config.json`

```json
{
    "runtime": {
        "runtime_mode": "Multi", // Multi, Single.
        "worker_threads": null, // null for default. only for Multi mode.
        "thread_stack_size": null, // null for default.
        "event_interval": null, // null for default.
        "global_queue_interval": null, // null for default.
        "max_io_events_per_tick": null, // null for default. only for Single mode.
        "thread_keep_alive": null // null for default. In seconds.
    },
    "log": {
        "level": "warn", // error, warn, info, debug, trace. set null to disable
        "file": "l.log" // set null to log to Stdout
    },
    "tls_buffer_limit": 64,
    "tcp_proxy_buffer_size": [8, 8], // The internal buffer size (read, write) for the TCP proxy. Unit is Kb.
    "udp_proxy_buffer_size": [8, 8], // The internal buffer size (read, write) for the UDP proxy. Unit is Kb.
    "tcp_idle_timeout": 150, // TCP idle timeout in seconds.
    "udp_idle_timeout": 90, // UDP idle timeout in seconds.
    "users": [ // User list
        {
            "name": "admin",
            "uuid": "a18b0775-2669-5cfc-b5e8-99bd5fd70884"
        }
    ],
    "inbounds":[
        // inbound objects
        {
            "listen": "0.0.0.0:80", // Server listening address and port. [::] works for both ipv4 and ipv6 in linux (dual stack).
            "transporter": "TCP", // Transport protocol
            "outbound": "direct", // default outbound tag
            "tls": { // TLS Configuration
                "enable": false, // Enable tls
                "max_fragment_size": null, // The maximum size of plaintext input to be emitted in a single TLS record. A value of null is equivalent to the TLS maximum of 16 kB. Unit is bytes.
                "alpn": ["h2", "http/1.1"],
                "certificate": "cert.pem", // Certificate Path
                "key": "key.pem" // Key Path
            }
        }
    ],
    "outbounds": { // Map object of outbounds
        "direct": {
            "opt": {
                "interface": null,
                "bind_to_device": false,
                "mss": null,
                "congestion": null,
                "send_buffer_size": null,
                "recv_buffer_size": null,
                "nodelay": null,
                "keepalive": null
            }
        }
    },
    "resolver": { // Built-in domain resolver supporting multiple protocols: udp, https, h3, tls, and quic
        "resolver": null, // 'null' or "udp://example" defaults to UDP; for other protocols, use: "https://dns.google", "h3://dns.google", "tls://dns.google", "quic://dns.google"
        "ips": ["1.1.1.1", "1.0.0.1", "2606:4700:4700::1111", "2606:4700:4700::1001"],
        "port": 53,
        "trust_negative_responses": true,
        "ip_strategy": "Ipv4thenIpv6", // Options: Ipv4Only, Ipv6Only, Ipv4AndIpv6, Ipv6thenIpv4, Ipv4thenIpv6,
        "cache_size": 64, // Cache size is in number of records
        "timeout": 2, // Specify the timeout for a request.
        "num_concurrent_reqs": 2 // Number of concurrent requests per query. Where more than one nameserver is configured, this configures the resolver to send queries to a number of servers in parallel. Defaults to 2; 0 or 1 will execute requests serially.
    },
    "rules": null
}
```

## Transports Configuration

TCP

```json
    "transporter": "TCP"
```

HTTP

```json
    "transporter": {
        "HTTP": {
            "path": "/",
            "host": "example.com", // If set null any host will be accepted
            "method": "GET"
        }
    }
```

HttpUpgrade

```json
    "transporter": {
        "HttpUpgrade": {
            "path": "/",
            "host": "example.com", // If set null any host will be accepted
            "method": "GET"
        }
    }
```

WS

```json
    "transporter": {
        "WS": {
            "path": "/",
            "host": "example.com", // If set null any host will be accepted.
            "frame_size": null // Max Outgoing frame size. The default is 1MB. Unit is Kb.
        }
    }
```

XHTTP

```json
    "transporter": {
        "XHTTP" : {
            "path": "/",
            "host": "example.com", // If set null any host will be accepted.
            "max_frame_size": 8, // must be less or equal to tcp_proxy_buffer_size. Unit is Kb.
            "max_send_buffer_size": 1024, // Sets the maximum send buffer size per stream. Unit is Kb.
            "initial_connection_window_size": 16384, // Indicates the initial window size (in octets) for connection-level flow control for received data. Unit is Kb.
            "initial_window_size": 1024 // Indicates the initial window size (in octets) for stream-level flow control for received data. Unit is Kb.
        }
    }
```

## Rules Configuration

```json
    "rules": [
        {
            "domains": [
                "google.com", // This will block any domain containing google.com for example mail.google.com.
                "googleadservices.com"
            ],
            "ips": [
                "192.168.0.1",
                "[2606:4700:4700::1111]"
            ],
            "operation": "Reject"
        },
        {
            "domains": null,
            "ips": [
                "8.8.8.8"
            ],
            "operation": {
                "Outbound": "outbound-tag"
            }
        },
    ]
```
