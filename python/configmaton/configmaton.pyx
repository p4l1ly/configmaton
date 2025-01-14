import sys
from libc.stddef cimport size_t
from cpython.ref cimport PyObject, Py_INCREF, Py_DECREF
from configmaton cimport c_configmaton

cdef class _Base:
    """Internal class for managing the lifetime of the Rust OwnedConfigmaton."""
    cdef c_configmaton.OwnedConfigmaton* _ptr

    def __dealloc__(self):
        c_configmaton.drop_configmaton_base(self._ptr)

cdef class Configmaton:
    """A configuration automaton that manages hierarchical configuration state."""

    cdef c_configmaton.FfiConfigmaton* _ptr
    cdef _Base _base
    cdef object _handle_commands

    def __init__(self, bytes buf not None, object handle_commands) -> 'Configmaton':
        """Create a new Configmaton instance.

        Args:
            buf: Initial buffer containing automaton definition.

        Returns:
            A new Configmaton instance.

        Raises:
            ValueError: If the buffer is invalid.
        """
        cdef _Base base = _Base()
        base._ptr = c_configmaton.new_configmaton_base(buf, len(buf))
        if base._ptr == NULL:
            raise ValueError("Failed to create configmaton from buffer")

        self._ptr = c_configmaton.base_get_configmaton(base._ptr)
        self._base = base
        self._handle_commands = handle_commands

    def make_child(self) -> 'Configmaton':
        """Create a child configmaton.

        Returns:
            A new child Configmaton instance.
        """
        cdef Configmaton child = Configmaton.__new__(Configmaton)
        child._ptr = c_configmaton.configmaton_make_child(self._ptr)
        child._base = self._base
        return child

    def set(self, bytes key not None, bytes value not None) -> None:
        """Set a configuration value.

        Args:
            key: Configuration key
            value: Configuration value
        """
        cdef c_configmaton.Bytestring cmd

        c_configmaton.configmaton_set(self._ptr, key, len(key), value, len(value))
        while True:
            cmd = c_configmaton.configmaton_pop_command(self._ptr)
            if cmd.len == sys.maxsize:
                return None
            return bytes(cmd.data[:cmd.len])

    def get(self, bytes key not None) -> Optional[bytes]:
        """Get a configuration value.

        Args:
            key: Configuration key
        """
        cdef c_configmaton.Bytestring result = c_configmaton.configmaton_get(
            self._ptr, key, len(key)
        )
        if result.len == sys.maxsize:
            return None
        return bytes(result.data[:result.len])
