#!/usr/bin/env python3

import socket

port = 9889

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(('', port))

while True:
    data, addr = sock.recvfrom(1024)
    print(data)
