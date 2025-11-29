import sys
from configmaton import Configmaton

c = Configmaton(sys.stdin.buffer.read(), lambda x: print(x.decode()))
c.set(b"foo", b"bar")
c.set(b"qux", b"ahoy")
print("LATER...")
c.set(b"foo", b"baz")
