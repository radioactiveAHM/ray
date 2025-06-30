# Ray

Vless server protocol written in rust. High performance, asynchronous, cheap and best for those who are avoiding complexity.

## Transports

- [x] HTTP
- [x] HTTP Upgrade
- [x] WS (WebSocket)
- [ ] XHTTP (Packet-up H2)
- [ ] XHTTP (Packet-up HTTP/1.1)

## Vless Request Commands

- [x] TCP
- [x] UDP
- [x] XUDP: Supporting complete udp mux.

## Configuration File `config.json`

**Notes:**

- Increasing the buffer size for `tcp_proxy_buffer_size` and `udp_proxy_buffer_size` enhances throughput and reduces latency and cpu usage. However, be mindful of memory usage and the number of users if the system runs out of memory, the application will crash.
- **Stack Proxy Method:** A built-in TCP proxy method that avoids buffering across multiple reads.

```json
{
    "log": false, // Enable logging. Disable for maximum performance
    "thread_stack_size": null, // The stack size (in bytes) for worker threads. The default stack size for spawned threads is 2 MiB. The actual stack size may be greater than this value if the platform specifies minimal stack size.
    "tcp_proxy_mod": "Stack", // The TCP proxy supports two algorithm variants: `Stack` and `Buffer`. The `Buffer` algorithm accumulates multiple incoming readings before processing, improving efficiency but potentially introducing slight latency. Conversely, the `Stack` algorithm processes data immediately upon arrival, minimizing delay.
    "tcp_proxy_buffer_size": null, // Defines the internal buffer size for the TCP proxy. If set to null, the buffer size defaults to 8KB. Unit is Kb.
    "udp_proxy_buffer_size": null, // Defines the internal buffer size for the UDP proxy. If set to null, the buffer size defaults to 8KB. Unit is Kb.
    "tcp_idle_timeout": 150, // TCP idle timeout in seconds (connection closes after 300 seconds of inactivity)
    "udp_idle_timeout": 90, // UDP idle timeout in seconds
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
        "address": null, // 'null' or "udp://example" defaults to UDP; for other protocols, use: "https://dns.google", "h3://dns.google", "tls://dns.google"
        "ip_port": "8.8.8.8:53", // Standard port: UDP (53), HTTPS (443), TLS/QUIC (853)
        "mode": "IPv4" // Options: 'IPv4' prioritizes IPv4 over IPv6; 'IPv6' prioritizes IPv6 over IPv4
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
            "host": "meow.com", // If set null any host will be accepted
            "method": "GET"
        }
    }
```

HttpUpgrade

```json
    "transporter": {
        "HttpUpgrade": {
            "path": "/",
            "host": "meow.com", // If set null any host will be accepted
            "method": "GET"
        }
    }
```

WS

```json
    "transporter": {
        "WS": {
            "path": "/",
            "host": "meow.com", // If set null any host will be accepted
            "threshold": null, // The default is 8 KiB. Unit is Kb. !!! It's better to set the value same as tcp_proxy_buffer_size.
            "frame_size": null // Max Outgoing frame size. The default is 4MiB. Unit is Kb.
        }
    }
```

XHttp

```json
    "transporter": {
        "XHttp": {
            "initial_connection_window_size": null,
            "initial_window_size": null,
            "max_concurrent_streams": null,
            "max_frame_size": null,
            "max_send_buffer_size": null,
            "reset_stream_duration": null,

            "recv_data_frame_timeout": 30000,
            "tcp_buffer_size": 8,

            "wait_for_sec_timeout": 30000,
            "wait_for_sec_interval": 100,

            "wait_for_init_timeout": 10000,
            "wait_for_init_interval": 50
        }
    }
```

## Blacklist Configuration

```json
    "blacklist": [
        { // Black list object
            "name": "google", // List name
            "domains": [ // List of domains
                "google.com",
                "www.google.com"
            ]
        },
        {
            "name": "facebook",
            "domains": [
                "facebook.com"
            ]
        },
    ]
```
