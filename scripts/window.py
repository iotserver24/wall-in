#!/usr/bin/python3
"""Open the Wall-in management interface in a native, unprivileged web view."""

import argparse
import sys
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://127.0.0.1:3977')
    args = parser.parse_args()
    parsed = urlsplit(args.url)
    if parsed.scheme != 'http' or parsed.hostname != '127.0.0.1' or parsed.username or parsed.password:
        parser.error('The management window only accepts an HTTP URL on 127.0.0.1.')
    origin = f'http://127.0.0.1:{parsed.port or 80}'
    for attempt in range(20):
        try:
            with urllib.request.urlopen(origin + '/api/health', timeout=2) as response:
                if response.status == 200:
                    break
        except urllib.error.URLError:
            pass
        time.sleep(0.5)
    else:
        print(f'Wall-in service is unavailable at {origin}. Start wall-in.service and retry.', file=sys.stderr)
        return 1

    try:
        import gi
        gi.require_version('Gtk', '4.0')
        gi.require_version('WebKit', '6.0')
        from gi.repository import Gio, Gtk, WebKit
    except (ImportError, ValueError) as error:
        print(f'Native window requires PyGObject, GTK 4 and WebKitGTK 6: {error}\nYou can open {origin} in your browser instead.', file=sys.stderr)
        return 1

    app = Gtk.Application(application_id='io.wallin.Manager', flags=Gio.ApplicationFlags.NON_UNIQUE)

    def activate(application):
        window = Gtk.ApplicationWindow(application=application, title='Wall-in')
        window.set_default_size(1360, 880)
        view = WebKit.WebView()
        settings = view.get_settings()
        settings.set_enable_developer_extras(False)
        settings.set_enable_write_console_messages_to_stdout(True)
        settings.set_media_playback_requires_user_gesture(True)

        def decide_policy(_view, decision, decision_type):
            if decision_type not in (WebKit.PolicyDecisionType.NAVIGATION_ACTION, WebKit.PolicyDecisionType.NEW_WINDOW_ACTION):
                return False
            uri = decision.get_navigation_action().get_request().get_uri()
            destination = urlsplit(uri)
            if (destination.scheme, destination.hostname, destination.port or 80) != ('http', '127.0.0.1', parsed.port or 80):
                decision.ignore()
                return True
            if decision_type == WebKit.PolicyDecisionType.NEW_WINDOW_ACTION:
                decision.ignore()
                return True
            return False

        view.connect('decide-policy', decide_policy)
        view.connect('load-failed', lambda _view, event, uri, error: print(f'WebKit load failed ({event}, {uri}): {error}', file=sys.stderr, flush=True))
        view.connect('web-process-terminated', lambda _view, reason: print(f'WebKit web process terminated: {reason}', file=sys.stderr, flush=True))
        view.load_uri(origin)
        window.set_child(view)
        window.present()

    app.connect('activate', activate)
    return app.run([sys.argv[0]])


if __name__ == '__main__':
    sys.exit(main())
