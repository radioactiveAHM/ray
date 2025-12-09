# Ray

> **Ray** is an asynchronous VLESS server protocol implemented in Rust.  
> _Note: This project is intended as a personal portfolio piece and is not actively maintained. Pull requests will not be accepted._

## Transports

- [x] HTTP
- [x] HTTP Upgrade
- [x] WS (WebSocket)

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
    "tcp_fill_buffer": false, // Enable multiple read attempts to fill the buffer.
    "tcp_proxy_buffer_size": null, // The internal buffer size for the TCP proxy. If set to null, the buffer size defaults to 8KB. Unit is Kb.
    "udp_proxy_buffer_size": null, // The internal buffer size for the UDP proxy. If set to null, the buffer size defaults to 8KB. Unit is Kb.
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
            "tls": { // TLS Configuration
                "enable": false, // Enable tls
                "max_fragment_size": null, // The maximum size of plaintext input to be emitted in a single TLS record. A value of null is equivalent to the TLS maximum of 16 kB.
                "alpn": ["h2", "http/1.1"],
                "certificate": "cert.pem", // Certificate Path
                "key": "key.pem" // Key Path
            },
            "sockopt": {
                "interface": null, // Bind interface/Adaptor
                "bind_to_device": false,
                "mss": null,
                "congestion": null
            }
        }
    ],
    "resolver": { // Built-in domain resolver supporting multiple protocols: udp, https, h3, tls, and quic
        "resolver": null, // 'null' or "udp://example" defaults to UDP; for other protocols, use: "https://dns.google", "h3://dns.google", "tls://dns.google", "quic://dns.google"
        "ips": ["1.1.1.1", "1.0.0.1", "2606:4700:4700::1111", "2606:4700:4700::1001"],
        "port": 53,
        "trust_negative_responses": true,
        "ip_strategy": "Ipv4thenIpv6", // Options: Ipv4Only, Ipv6Only, Ipv4AndIpv6, Ipv6thenIpv4, Ipv4thenIpv6,
        "cache_size": 64,
        "timeout": 5,
        "num_concurrent_reqs": 2
    },
    "tcp_socket_options": {
        "send_buffer_size": null, // The size of the socket send buffer, if set; null means default system size
        "recv_buffer_size": null, // The size of the socket receive buffer, if set; null means default system size
        "nodelay": null, // Whether to disable Nagle’s algorithm; false means packets may be buffered for efficiency
        "keepalive": null, // Whether to enable keepalive packets to maintain connection activity
    },
    "blacklist": null // Domain blacklist
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

## Blacklist Configuration

```json
    "blacklist": [
        { // Black list object
            "name": "google",
            "domains": [ // List of domains
                "google.com", // This will block any domain containing google.com for example mail.google.com.
                "googleadservices.com"
            ]
        },
        {
            "name": "xyz blacklist",
            "domains": [
                ".xyz" // This will block any domain xyz domain.
            ]
        },
    ]
```
