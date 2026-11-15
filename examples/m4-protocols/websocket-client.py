"""Capture one protocol-1 PNG frame using Python 3 standard library only."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
from urllib.parse import urlsplit


def capture(url, output):
    target = urlsplit(url)
    if target.scheme != "ws":
        raise ValueError("This local demo accepts ws:// URLs")
    with socket.create_connection((target.hostname, target.port or 80), timeout=15) as connection:
        key = base64.b64encode(os.urandom(16)).decode()
        connection.sendall((f"GET {target.path} HTTP/1.1\r\nHost: {target.netloc}\r\n"
            f"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n"
            f"Sec-WebSocket-Key: {key}\r\n\r\n").encode())
        stream = connection.makefile("rb")
        if not stream.readline().startswith(b"HTTP/1.1 101 "):
            raise RuntimeError("WebSocket upgrade failed")
        headers = {}
        while (line := stream.readline()) != b"\r\n":
            if not line:
                raise EOFError("Incomplete upgrade")
            name, value = line.decode().split(":", 1)
            headers[name.lower()] = value.strip()
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        if headers.get("sec-websocket-accept") != accept:
            raise RuntimeError("Invalid WebSocket acceptance")

        def read_exact(size):
            data = stream.read(size)
            if len(data) != size:
                raise EOFError("Incomplete WebSocket message")
            return data

        def receive():
            first, second = read_exact(2)
            if not first & 0x80 or second & 0x80:
                raise ValueError("Unexpected fragmented or masked server frame")
            length = second & 127
            if length == 126:
                length = struct.unpack("!H", read_exact(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", read_exact(8))[0]
            if length > 32 * 1024 * 1024:
                raise ValueError("Demo frame limit exceeded")
            return first & 15, read_exact(length)

        def send(opcode, data):
            mask = os.urandom(4)
            length = len(data)
            header = bytes([0x80 | opcode, 0x80 | length]) if length < 126 else bytes([0x80 | opcode, 0x80 | 126]) + struct.pack("!H", length)
            connection.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

        opcode, data = receive()
        hello = json.loads(data)
        if opcode != 1 or hello.get("type") != "hello" or hello.get("protocol") != 1:
            raise ValueError("Expected protocol-1 hello")
        send(1, json.dumps({"type": "init", "request_id": "demo-init",
        "view": {"crs": "OGC:CRS84", "bbox": [110, 30, 125, 45], "width": 512, "height": 512},
        "variant": {"id": "default"},
        "playback": {"time": 0, "speed": 1, "playing": False},
        "output": {"format": "png", "fps": 30}}).encode())
        opcode, data = receive()
        ack = json.loads(data)
        if opcode != 1 or ack.get("type") != "ack":
            raise ValueError(f"Expected ack: {ack}")
        opcode, data = receive()
        metadata = json.loads(data)
        if opcode != 1 or metadata.get("type") != "frame" or metadata["revision"] != ack["revision"]:
            raise ValueError(f"Expected matching frame: {metadata}")
        opcode, data = receive()
        if opcode != 2 or len(data) != metadata["bytes"] or not data.startswith(b"\x89PNG\r\n\x1a\n"):
            raise ValueError("Image does not match frame metadata")
        Path(output).write_bytes(data)
        Path(str(output) + ".json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        send(8, struct.pack("!H", 1000))
        print(f"Captured {len(data)} bytes: {output}")
        return metadata


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="ws://127.0.0.1:3000/ws/maps/demo:labels")
    parser.add_argument("--output", default="frame.png")
    args = parser.parse_args()
    capture(args.url, args.output)
