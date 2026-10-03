"""Experimental EWMH desktop hints, applied before GTK maps its X11 surface."""

import ctypes as C
import ctypes.util
from contextlib import contextmanager


class WMHints(C.Structure):
    _fields_ = [("flags", C.c_long), ("input", C.c_int), ("initial_state", C.c_int),
                ("icon_pixmap", C.c_ulong), ("icon_window", C.c_ulong),
                ("icon_x", C.c_int), ("icon_y", C.c_int), ("icon_mask", C.c_ulong),
                ("window_group", C.c_ulong)]


@contextmanager
def connection():
    library = ctypes.util.find_library("X11")
    if not library:
        raise RuntimeError("libX11 is unavailable")
    x = C.CDLL(library)
    signatures = {
        "XOpenDisplay": ([C.c_char_p], C.c_void_p),
        "XCloseDisplay": ([C.c_void_p], C.c_int),
        "XDefaultRootWindow": ([C.c_void_p], C.c_ulong),
        "XInternAtom": ([C.c_void_p, C.c_char_p, C.c_int], C.c_ulong),
        "XChangeProperty": ([C.c_void_p, C.c_ulong, C.c_ulong, C.c_ulong, C.c_int,
                             C.c_int, C.c_void_p, C.c_int], C.c_int),
        "XGetWindowProperty": ([C.c_void_p, C.c_ulong, C.c_ulong, C.c_long, C.c_long,
                                C.c_int, C.c_ulong, C.POINTER(C.c_ulong), C.POINTER(C.c_int),
                                C.POINTER(C.c_ulong), C.POINTER(C.c_ulong),
                                C.POINTER(C.POINTER(C.c_ubyte))], C.c_int),
        "XFree": ([C.c_void_p], C.c_int),
        "XSetWMHints": ([C.c_void_p, C.c_ulong, C.POINTER(WMHints)], C.c_int),
        "XMoveResizeWindow": ([C.c_void_p, C.c_ulong, C.c_int, C.c_int,
                               C.c_uint, C.c_uint], C.c_int),
        "XLowerWindow": ([C.c_void_p, C.c_ulong], C.c_int),
        "XSync": ([C.c_void_p, C.c_int], C.c_int),
    }
    for name, (arguments, result) in signatures.items():
        function = getattr(x, name)
        function.argtypes = arguments
        function.restype = result
    display = x.XOpenDisplay(None)
    if not display:
        raise RuntimeError("Cannot connect to the X11 display")
    try:
        yield x, display
    finally:
        x.XCloseDisplay(display)


def atom(x, display, name):
    return x.XInternAtom(display, name.encode("ascii"), False)


def check_support():
    import gi
    gi.require_version("GdkX11", "4.0")
    from gi.repository import GdkX11
    if not hasattr(GdkX11, "X11Surface"):
        raise RuntimeError("GDK X11 surface support is unavailable")
    with connection() as (x, display):
        actual_type, actual_format = C.c_ulong(), C.c_int()
        count, remaining = C.c_ulong(), C.c_ulong()
        data = C.POINTER(C.c_ubyte)()
        # only_if_exists keeps diagnostics from creating atoms on the X server.
        property_atom = x.XInternAtom(display, b"_NET_SUPPORTED", True)
        if not property_atom:
            raise RuntimeError("X11 window manager does not advertise EWMH support")
        status = x.XGetWindowProperty(display, x.XDefaultRootWindow(display), property_atom,
                                      0, 4096, False, 4, C.byref(actual_type),
                                      C.byref(actual_format), C.byref(count), C.byref(remaining),
                                      C.byref(data))
        try:
            if status != 0 or actual_format.value != 32 or actual_type.value != 4:
                raise RuntimeError("X11 window manager has no valid _NET_SUPPORTED property")
            supported = set(C.cast(data, C.POINTER(C.c_ulong))[:count.value])
            for name in ("_NET_WM_WINDOW_TYPE_DESKTOP", "_NET_WM_STATE_BELOW",
                         "_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER", "_NET_WM_DESKTOP"):
                if x.XInternAtom(display, name.encode("ascii"), True) not in supported:
                    raise RuntimeError(f"X11 window manager does not advertise {name}")
        finally:
            if data:
                x.XFree(data)


def configure_window(surface, geometry):
    import gi
    gi.require_version("GdkX11", "4.0")
    from gi.repository import GdkX11
    if not isinstance(surface, GdkX11.X11Surface):
        raise RuntimeError("Expected a native X11 surface")
    xid = surface.get_xid()
    with connection() as (x, display):
        def prop(name, type_name, values):
            data = (C.c_ulong * len(values))(*values)
            x.XChangeProperty(display, xid, atom(x, display, name), atom(x, display, type_name),
                              32, 0, data, len(values))

        prop("_NET_WM_WINDOW_TYPE", "ATOM", [atom(x, display, "_NET_WM_WINDOW_TYPE_DESKTOP")])
        prop("_NET_WM_STATE", "ATOM", [atom(x, display, state) for state in
             ("_NET_WM_STATE_BELOW", "_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER")])
        prop("_NET_WM_DESKTOP", "CARDINAL", [0xFFFFFFFF])
        hints = WMHints(flags=1, input=0)
        x.XSetWMHints(display, xid, C.byref(hints))
        scale = surface.get_scale_factor()
        x.XMoveResizeWindow(display, xid, geometry.x * scale, geometry.y * scale,
                           geometry.width * scale, geometry.height * scale)
        x.XLowerWindow(display, xid)
        x.XSync(display, False)
