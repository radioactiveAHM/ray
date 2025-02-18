# Ray

Vless server protocol written in rust. High performance, asynchronous, cheap and best for those who are avoiding complexity.

## Transports

- [x] HTTP
- [x] HTTP Upgrade

## Vless Request Commands

- [x] TCP 0x01
- [x] DNS UDP 0x02
- [x] MUX 0x03 (Only for UDP)

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
