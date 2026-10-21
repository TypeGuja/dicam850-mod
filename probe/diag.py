import socket
for p in [80, 554, 15740, 23]:
    s = socket.socket(); s.settimeout(2.0); s.bind(("192.168.1.10", 0))
    try:
        s.connect(("192.168.1.1", p)); print("OPEN", p)
    except OSError as e:
        print("port", p, type(e).__name__, e)
    finally:
        s.close()
