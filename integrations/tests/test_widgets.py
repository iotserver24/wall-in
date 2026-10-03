import json
import unittest
from unittest.mock import Mock
from types import SimpleNamespace as NS

from integrations import widgets


class WidgetBridgeTests(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self.shell_owner = ':1.17'

        def call(name, _path, _iface, method, *args):
            self.calls.append(method)
            if method == 'GetNameOwner':
                return NS(unpack=lambda: (self.shell_owner,))
            if method == 'Probe':
                return NS(unpack=lambda: (json.dumps({'version': 2, 'widgets': {'version': 1}}),))
            if method == 'SetWidgets':
                return NS(unpack=lambda: (True,))
            return NS(unpack=lambda: ())

        conn = NS(call_sync=call)
        glib = NS(Variant=lambda _signature, value: value, VariantType=lambda signature: signature,
                  Error=RuntimeError)
        gio = NS(DBusCallFlags=NS(NONE=0))
        self.bridge = widgets.WidgetBridge(conn, glib, gio)

    def test_set_keeps_sender_connection_and_reapplies_after_shell_change(self):
        self.bridge.available()
        self.bridge.set([{'id': 'clock', 'text': '12:34'}])
        self.assertEqual(self.calls.count('SetWidgets'), 1)
        self.bridge.reconnect()
        self.assertEqual(self.calls.count('SetWidgets'), 1)
        self.shell_owner = ':1.88'
        self.bridge.reconnect()
        self.assertEqual(self.calls.count('SetWidgets'), 2)
        self.bridge.stop()
        self.assertIn('ClearWidgets', self.calls)

    def test_rejects_large_documents_and_many_widgets(self):
        self.bridge.available()
        with self.assertRaisesRegex(ValueError, '32 widgets'):
            self.bridge.set([{}] * 33)
        with self.assertRaisesRegex(ValueError, '512 KiB'):
            self.bridge.set([{'text': 'x' * widgets.MAX_LINE}])
        self.assertNotIn('SetWidgets', self.calls)


if __name__ == '__main__':
    unittest.main()
