# Ray

Vless server protocol written in rust. High performance, asynchronous, cheap and best for those who are avoiding complexity.

## Transports

- [x] HTTP
- [x] HTTP Upgrade

## Vless Request Commands

- [x] TCP 0x01
- [x] DNS UDP 0x02
- [x] XUDP: Functional, but lacks mux support.

## Configuration File `config.json`

```json
{
    "log": false, // Enable logging. Disable for maximum performance
    "tcp_idle_timeout": 300, // TCP idle timeout in seconds (connection closes after 300 seconds of inactivity)
    "udp_idle_timeout": 90, // UDP idle timeout in seconds
    "listen": "[::]:80", // Server listening address and port. [::] works for both ipv4 and ipv6 in linux.
    "users": [ // User list
        {
            "name": "admin",
            "uuid": "a18b0775-2669-5cfc-b5e8-99bd5fd70884"
        }
    ],
    "transporter": "TCP", // Transport protocol
    "tls": { // TLS Configuration
        "enable": false, // Enable tls
        "alpn": ["h2", "http/1.1"],
        "certificate": "cert.pem", // Certificate Path
        "key": "key.pem" // Key Path
    },
    "resolver": { // Built-in domain resolver supporting multiple protocols: udp, https, h3, tls, and quic
        "address": null, // 'null' or "udp://example" defaults to UDP; for other protocols, use: "https://dns.google", "h3://dns.google", "tls://dns.google"
        "ip_port": "1.1.1.1:53", // Standard port: UDP (53), HTTPS (443), TLS/QUIC (853)
        "mode": "IPv4" // Options: 'IPv4' prioritizes IPv4 over IPv6; 'IPv6' prioritizes IPv6 over IPv4
    },
    "tcp_socket_options": {
        "send_buffer_size": null, // The size of the socket send buffer, if set; null means default system size
        "recv_buffer_size": null, // The size of the socket receive buffer, if set; null means default system size
        "nodelay": false, // Whether to disable Nagle’s algorithm; false means packets may be buffered for efficiency
        "keepalive": true, // Whether to enable keepalive packets to maintain connection activity
        "listen_backlog": 4096 // Maximum number of queued connections waiting to be accepted
    }
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
