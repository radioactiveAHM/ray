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
            "host": "meow.com",
            "method": "GET"
        }
    }
```

HttpUpgrade

```json
    "transporter": {
        "HttpUpgrade": {
            "path": "/",
            "host": "meow.com",
            "method": "GET"
        }
    }
```
