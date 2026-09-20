"""One persistent XDG RemoteDesktop + ScreenCast + PipeWire session."""

from __future__ import annotations

import os
import sys
import threading
import uuid
from pathlib import Path
from typing import Any

# Ubuntu packages python3-dbus/python3-gi live here. A venv created without
# --system-site-packages hides them, although they are required for PipeWire
# portal access and are not practical pure-Python pip dependencies.
_ubuntu_dist_packages = Path("/usr/lib/python3/dist-packages")
if _ubuntu_dist_packages.is_dir() and str(_ubuntu_dist_packages) not in sys.path:
    sys.path.append(str(_ubuntu_dist_packages))

try:
    import dbus
    from dbus.mainloop.glib import DBusGMainLoop
    import gi
except ModuleNotFoundError as error:
    raise RuntimeError(
        "Live mode needs Ubuntu's python3-dbus and python3-gi packages. "
        "Install them, or recreate the venv with: python3 -m venv --system-site-packages .venv"
    ) from error
from PIL import Image

from input import CommandResult, InputExecutionError

gi.require_version("Gst", "1.0")
from gi.repository import GLib, Gst  # noqa: E402


PORTAL_BUS = "org.freedesktop.portal.Desktop"
PORTAL_PATH = "/org/freedesktop/portal/desktop"
DEVICE_KEYBOARD = 1
DEVICE_POINTER = 2
SOURCE_MONITOR = 1
CURSOR_EMBEDDED = 2
BTN_LEFT = 0x110  # Linux evdev BTN_LEFT
KEYSYM_RETURN = 0xFF0D


class PortalError(RuntimeError):
    pass


def _token() -> str:
    return f"vocs{uuid.uuid4().hex}"


class PortalSession:
    """Keeps the portal session and PipeWire stream alive until ``close``."""

    def __init__(self) -> None:
        DBusGMainLoop(set_as_default=True)
        Gst.init(None)
        self.bus = dbus.SessionBus()
        self.desktop = self.bus.get_object(PORTAL_BUS, PORTAL_PATH)
        self.remote = dbus.Interface(self.desktop, "org.freedesktop.portal.RemoteDesktop")
        self.screencast = dbus.Interface(self.desktop, "org.freedesktop.portal.ScreenCast")
        self.session_handle: dbus.ObjectPath | None = None
        self.stream_id: int | None = None
        self._logical_size: tuple[int, int] | None = None
        self._latest: Image.Image | None = None
        self._condition = threading.Condition()
        self._pipeline: Gst.Pipeline | None = None
        self._pipewire_fd: int | None = None

    def _request(self, request_handle: dbus.ObjectPath) -> dict[str, Any]:
        response: dict[str, Any] = {}
        loop = GLib.MainLoop()

        def on_response(code: int, results: dict[str, Any]) -> None:
            response["code"] = int(code)
            response["results"] = dict(results)
            loop.quit()

        self.bus.add_signal_receiver(
            on_response,
            signal_name="Response",
            dbus_interface="org.freedesktop.portal.Request",
            path=str(request_handle),
        )
        loop.run()
        self.bus.remove_signal_receiver(
            on_response,
            signal_name="Response",
            dbus_interface="org.freedesktop.portal.Request",
            path=str(request_handle),
        )
        if response.get("code") != 0:
            raise PortalError(f"Portal request was cancelled or failed (response={response.get('code')})")
        return response["results"]

    @staticmethod
    def _options(**values: Any) -> dbus.Dictionary:
        return dbus.Dictionary(values, signature="sv")

    @staticmethod
    def _size(value: Any) -> tuple[int, int] | None:
        if isinstance(value, (tuple, list, dbus.Struct)) and len(value) >= 2:
            width, height = int(value[0]), int(value[1])
            if width > 0 and height > 0:
                return width, height
        return None

    def start(self) -> None:
        """Show portal consent once, then begin consuming its single PipeWire stream."""
        created = self._request(self.remote.CreateSession(self._options(
            handle_token=dbus.String(_token()), session_handle_token=dbus.String(_token()),
        )))
        self.session_handle = created["session_handle"]
        self._request(self.remote.SelectDevices(self.session_handle, self._options(
            handle_token=dbus.String(_token()), types=dbus.UInt32(DEVICE_POINTER | DEVICE_KEYBOARD),
        )))
        self._request(self.screencast.SelectSources(self.session_handle, self._options(
            handle_token=dbus.String(_token()), types=dbus.UInt32(SOURCE_MONITOR),
            multiple=dbus.Boolean(False), cursor_mode=dbus.UInt32(CURSOR_EMBEDDED),
        )))
        # In a combined RemoteDesktop/ScreenCast session, this is the one Start call.
        started = self._request(self.remote.Start(self.session_handle, "", self._options(handle_token=dbus.String(_token()))))
        streams = started.get("streams", [])
        if len(streams) != 1:
            raise PortalError(f"Expected exactly one monitor stream, got {len(streams)}")
        self.stream_id, properties = int(streams[0][0]), dict(streams[0][1])
        self._logical_size = self._size(properties.get("logical_size")) or self._size(properties.get("size"))
        unix_fd = self.screencast.OpenPipeWireRemote(self.session_handle, self._options())
        self._pipewire_fd = unix_fd.take() if hasattr(unix_fd, "take") else int(unix_fd)
        self._start_pipewire(self._pipewire_fd, self.stream_id)

    def _start_pipewire(self, fd: int, stream_id: int) -> None:
        pipeline = Gst.parse_launch(
            f"pipewiresrc fd={fd} path={stream_id} ! videoconvert ! "
            "video/x-raw,format=RGB ! appsink name=latest emit-signals=true max-buffers=1 drop=true sync=false"
        )
        sink = pipeline.get_by_name("latest")
        sink.connect("new-sample", self._on_sample)
        if pipeline.set_state(Gst.State.PLAYING) == Gst.StateChangeReturn.FAILURE:
            raise PortalError("Could not start the PipeWire GStreamer pipeline")
        self._pipeline = pipeline

    def _on_sample(self, sink: Gst.Element) -> Gst.FlowReturn:
        sample = sink.emit("pull-sample")
        if sample is None:
            return Gst.FlowReturn.ERROR
        caps = sample.get_caps().get_structure(0)
        width, height = caps.get_value("width"), caps.get_value("height")
        buffer = sample.get_buffer()
        success, mapped = buffer.map(Gst.MapFlags.READ)
        if not success:
            return Gst.FlowReturn.ERROR
        try:
            image = Image.frombytes("RGB", (width, height), bytes(mapped.data))
        finally:
            buffer.unmap(mapped)
        with self._condition:
            self._latest = image
            if self._logical_size is None:
                self._logical_size = image.size
            self._condition.notify_all()
        return Gst.FlowReturn.OK

    @property
    def stream_logical_size(self) -> tuple[int, int]:
        if self._logical_size is None:
            raise PortalError("PipeWire stream has not produced a frame yet")
        return self._logical_size

    def capture_latest(self, timeout_seconds: float = 10.0) -> Image.Image:
        with self._condition:
            if self._latest is None and not self._condition.wait_for(lambda: self._latest is not None, timeout_seconds):
                raise PortalError("Timed out waiting for the first PipeWire frame")
            assert self._latest is not None
            return self._latest.copy()

    def close(self) -> None:
        if self._pipeline is not None:
            self._pipeline.set_state(Gst.State.NULL)
            self._pipeline = None
        if self._pipewire_fd is not None:
            try:
                os.close(self._pipewire_fd)
            except OSError:
                pass
            self._pipewire_fd = None
        if self.session_handle is not None:
            try:
                session = self.bus.get_object(PORTAL_BUS, self.session_handle)
                dbus.Interface(session, "org.freedesktop.portal.Session").Close()
            except dbus.DBusException:
                pass
            self.session_handle = None

    def __enter__(self) -> PortalSession:
        self.start()
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


class PortalInput:
    """Input executor for an already-authorized RemoteDesktop session."""

    def __init__(self, portal: PortalSession, execute: bool) -> None:
        self.portal, self.execute = portal, execute

    def _call(self, method: str, *args: Any) -> CommandResult:
        command = ["portal", method]
        if not self.execute:
            return CommandResult(command, "", "", None, False)
        try:
            getattr(self.portal.remote, method)(self.portal.session_handle, dbus.Dictionary({}, signature="sv"), *args)
        except dbus.DBusException as error:
            result = CommandResult(command, "", str(error), 1, True)
            raise InputExecutionError(f"portal {method} failed", [result]) from error
        return CommandResult(command, "", "", 0, True)

    def click(self, x: int, y: int) -> list[CommandResult]:
        if self.portal.stream_id is None:
            raise InputExecutionError("Portal stream is unavailable", [])
        results = [self._call("NotifyPointerMotionAbsolute", dbus.UInt32(self.portal.stream_id), float(x), float(y))]
        results.append(self._call("NotifyPointerButton", dbus.Int32(BTN_LEFT), dbus.UInt32(1)))
        results.append(self._call("NotifyPointerButton", dbus.Int32(BTN_LEFT), dbus.UInt32(0)))
        return results

    def type_text(self, text: str) -> list[CommandResult]:
        results: list[CommandResult] = []
        for character in text:
            keysym = dbus.Int32(ord(character))
            results.append(self._call("NotifyKeyboardKeysym", keysym, dbus.UInt32(1)))
            results.append(self._call("NotifyKeyboardKeysym", keysym, dbus.UInt32(0)))
        return results

    def press_enter(self) -> list[CommandResult]:
        return [
            self._call("NotifyKeyboardKeysym", dbus.Int32(KEYSYM_RETURN), dbus.UInt32(1)),
            self._call("NotifyKeyboardKeysym", dbus.Int32(KEYSYM_RETURN), dbus.UInt32(0)),
        ]
