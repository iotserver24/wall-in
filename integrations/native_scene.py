"""Bounded Wall-in v1 cube scenes rendered through GTK's own GL context."""

import ctypes as C
import json
import math
from pathlib import Path
import time

MAX_BYTES = 4096
FACES = ((0, 1, 2, 3), (5, 4, 7, 6), (4, 0, 3, 7),
         (1, 5, 6, 2), (3, 2, 6, 7), (4, 5, 1, 0))
CORNERS = ((-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1),
           (-1, -1, -1), (1, -1, -1), (1, 1, -1), (-1, 1, -1))


def load(path):
    path = Path(path)
    if not str(path).endswith('.wallin-scene.json') or not path.is_file():
        raise ValueError('Native scene requires .wallin-scene.json')
    with path.open('rb') as stream:
        data = stream.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError('Native scene exceeds 4096 bytes')

    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('Duplicate native scene field')
            result[key] = value
        return result

    scene = json.loads(data, object_pairs_hook=unique, parse_constant=lambda _: (_ for _ in ()).throw(ValueError('Nonfinite number')))
    def fields(obj, names):
        if type(obj) is not dict or obj.keys() != set(names):
            raise ValueError('Unknown or missing native scene field')
    def number(value, low, high):
        if type(value) not in (int, float) or not low <= value <= high or not math.isfinite(value):
            raise ValueError('Native scene number out of range')
    def color(value):
        if (type(value) is not str or len(value) != 7 or value[0] != '#' or
                any(c not in '0123456789abcdefABCDEF' for c in value[1:])):
            raise ValueError('Invalid native scene color')
        return tuple(int(value[i:i + 2], 16) / 255 for i in (1, 3, 5))

    fields(scene, ('version', 'camera', 'cube', 'background'))
    if type(scene['version']) is not int or scene['version'] != 1:
        raise ValueError('Unsupported native scene version')
    camera, cube = scene['camera'], scene['cube']
    fields(camera, ('fov', 'distance'))
    fields(cube, ('size', 'rpm', 'colors'))
    number(camera['fov'], 30, 100)
    number(camera['distance'], 3, 20)
    number(cube['size'], .2, 2)
    number(cube['rpm'], 1, 60)
    if cube['size'] * 1.74 >= camera['distance'] - .1:
        raise ValueError('Cube intersects camera')
    if type(cube['colors']) is not list or len(cube['colors']) != 6:
        raise ValueError('Native scene requires six face colors')
    return {'fov': camera['fov'], 'distance': camera['distance'], 'size': cube['size'],
            'rpm': cube['rpm'], 'colors': tuple(map(color, cube['colors'])),
            'background': color(scene['background'])}


def vertices(scene, width, height, angle):
    if width <= 0 or height <= 0:
        raise ValueError('Invalid GL viewport')
    co, si = math.cos(angle), math.sin(angle)
    cx, sx = math.cos(angle * .67), math.sin(angle * .67)
    scale = scene['size'] / 2
    focal = 1 / math.tan(math.radians(scene['fov']) / 2)
    positions = []
    for x, y, z in CORNERS:
        x, y, z = x * scale, y * scale, z * scale
        x, z = co * x + si * z, -si * x + co * z
        y, z = cx * y - sx * z, sx * y + cx * z
        z -= scene['distance']
        positions.append((x * focal * height / width, y * focal,
                          (20.1 / -19.9) * z - 4 / 19.9, -z))
    result = []
    for face, color in zip(FACES, scene['colors']):
        for index in (face[0], face[1], face[2], face[0], face[2], face[3]):
            result.extend((*positions[index], *color))
    return result


class CubeArea:
    def __init__(self, Gtk, GLib, scene, fail):
        self.scene = scene
        self.fail = fail
        self.area = Gtk.GLArea(hexpand=True, vexpand=True)
        self.area.set_has_depth_buffer(True)
        self.area.set_can_target(False)
        self.area.connect('realize', self.realize)
        self.area.connect('render', self.render)
        self.area.connect('unrealize', self.unrealize)
        self.GLib = GLib
        self.gl = None
        self.program = 0
        self.buffer = C.c_uint()
        self.rendered = False
        self.paused = False
        self.elapsed = 0.0
        self.last = time.monotonic()
        self.timer = GLib.timeout_add(33, self.tick)

    def tick(self):
        if not self.paused:
            self.area.queue_render()
        return True

    def set_paused(self, paused):
        if paused == self.paused:
            return
        now = time.monotonic()
        if not self.paused:
            self.elapsed += now - self.last
        self.last = now
        self.paused = paused
        if not paused:
            self.area.queue_render()

    def stop(self):
        if self.timer:
            self.GLib.source_remove(self.timer)
            self.timer = None
        self.paused = True

    def realize(self, area):
        try:
            area.make_current()
            if area.get_error():
                raise RuntimeError(area.get_error().message)
            es = area.get_context().get_use_es()
            gl = C.CDLL('libGLESv2.so.2' if es else 'libGL.so.1')
            def fn(name, restype, *argtypes):
                function = getattr(gl, name)
                function.restype = restype
                function.argtypes = argtypes
                return function
            U, I, P = C.c_uint, C.c_int, C.c_void_p
            self.create_shader = fn('glCreateShader', U, U)
            self.shader_source = fn('glShaderSource', None, U, I, P, P)
            self.compile_shader = fn('glCompileShader', None, U)
            self.get_shader = fn('glGetShaderiv', None, U, U, P)
            self.create_program = fn('glCreateProgram', U)
            self.attach_shader = fn('glAttachShader', None, U, U)
            self.bind_attrib = fn('glBindAttribLocation', None, U, U, C.c_char_p)
            self.link_program = fn('glLinkProgram', None, U)
            self.get_program = fn('glGetProgramiv', None, U, U, P)
            self.delete_shader = fn('glDeleteShader', None, U)
            self.delete_program = fn('glDeleteProgram', None, U)
            self.gen_buffers = fn('glGenBuffers', None, I, P)
            self.delete_buffers = fn('glDeleteBuffers', None, I, P)
            self.bind_buffer = fn('glBindBuffer', None, U, U)
            self.buffer_data = fn('glBufferData', None, U, C.c_size_t, P, U)
            self.use_program = fn('glUseProgram', None, U)
            self.vertex_attrib = fn('glVertexAttribPointer', None, U, I, U, C.c_ubyte, I, P)
            self.enable_attrib = fn('glEnableVertexAttribArray', None, U)
            self.viewport = fn('glViewport', None, I, I, I, I)
            self.clear_color = fn('glClearColor', None, C.c_float, C.c_float, C.c_float, C.c_float)
            self.clear = fn('glClear', None, U)
            self.enable = fn('glEnable', None, U)
            self.draw = fn('glDrawArrays', None, U, I, I)
            self.get_error = fn('glGetError', U)
            prefix = b'#version 100\nprecision highp float;\n' if es else b'#version 120\n'
            def shader(kind, source):
                handle = self.create_shader(kind)
                code = C.c_char_p(prefix + source)
                self.shader_source(handle, 1, C.byref(code), None)
                self.compile_shader(handle)
                ok = I()
                self.get_shader(handle, 0x8B81, C.byref(ok))
                if not ok.value:
                    raise RuntimeError('Built-in cube shader compilation failed')
                return handle
            vertex = shader(0x8B31, b'attribute vec4 position; attribute vec3 color; varying vec3 shade; void main() { gl_Position = position; shade = color; }')
            fragment = shader(0x8B30, b'varying vec3 shade; void main() { gl_FragColor = vec4(shade, 1.0); }')
            self.program = self.create_program()
            self.attach_shader(self.program, vertex)
            self.attach_shader(self.program, fragment)
            self.bind_attrib(self.program, 0, b'position')
            self.bind_attrib(self.program, 1, b'color')
            self.link_program(self.program)
            ok = I()
            self.get_program(self.program, 0x8B82, C.byref(ok))
            self.delete_shader(vertex)
            self.delete_shader(fragment)
            if not ok.value:
                raise RuntimeError('Built-in cube shader link failed')
            self.gen_buffers(1, C.byref(self.buffer))
            self.gl = gl
        except Exception as error:
            self.fail(error)

    def render(self, area, _context):
        if not self.gl:
            return False
        try:
            width, height = area.get_width() * area.get_scale_factor(), area.get_height() * area.get_scale_factor()
            if not width or not height:
                return False
            if not self.paused:
                now = time.monotonic()
                self.elapsed += now - self.last
                self.last = now
            angle = self.elapsed * self.scene['rpm'] * math.tau / 60
            data = (C.c_float * (36 * 7))(*vertices(self.scene, width, height, angle))
            self.viewport(0, 0, width, height)
            self.clear_color(*self.scene['background'], 1)
            self.clear(0x4000 | 0x0100)
            self.enable(0x0B71)
            self.use_program(self.program)
            self.bind_buffer(0x8892, self.buffer.value)
            self.buffer_data(0x8892, C.sizeof(data), data, 0x88E0)
            for index, count, offset in ((0, 4, 0), (1, 3, 16)):
                self.enable_attrib(index)
                self.vertex_attrib(index, count, 0x1406, 0, 28, C.c_void_p(offset))
            self.draw(0x0004, 0, 36)
            if self.get_error():
                raise RuntimeError('Native scene OpenGL rendering failed')
            self.rendered = True
            return True
        except Exception as error:
            self.fail(error)
            return False

    def unrealize(self, area):
        if self.gl:
            area.make_current()
            if self.buffer.value:
                self.delete_buffers(1, C.byref(self.buffer))
            if self.program:
                self.delete_program(self.program)
            self.gl = None
