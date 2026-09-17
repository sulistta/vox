#!/usr/bin/env python3
"""Small, deliberately safe GTK fixture for native accessibility tests.

It has no network, filesystem effects, credentials, or data from the user's
desktop. The native integration test starts and terminates this process while
it verifies that Vox can observe and safely invoke AT-SPI actions.
"""

from __future__ import annotations

import argparse
import pathlib
import signal

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402


class AccessibilityFixture:
    def __init__(self, ready_file: pathlib.Path) -> None:
        self.counter = 0
        self.recreated = 0
        self.ready_file = ready_file

        self.window = Gtk.Window(title="Vox Accessibility Fixture")
        self.window.set_default_size(480, 360)
        self.window.set_border_width(18)
        self.window.set_resizable(True)
        self.window.connect("destroy", Gtk.main_quit)

        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.window.add(content)

        heading = Gtk.Label(label="Native accessibility test fixture")
        heading.set_xalign(0)
        content.pack_start(heading, False, False, 0)

        self.status = Gtk.Label(label="Action count: 0")
        self.status.set_xalign(0)
        content.pack_start(self.status, False, False, 0)

        self.dialog_status = Gtk.Label(label="Dialog state: closed")
        self.dialog_status.set_xalign(0)
        content.pack_start(self.dialog_status, False, False, 0)

        safe_action = Gtk.Button.new_with_label("Safe action")
        safe_action.connect("clicked", self.increment_action)
        content.pack_start(safe_action, False, False, 0)

        # Two controls deliberately share the same accessible name. A caller
        # must treat a selector for this name as ambiguous instead of choosing
        # whichever element happens to be visited first.
        duplicate_action_a = Gtk.Button.new_with_label("Duplicate action")
        duplicate_action_a.connect("clicked", self.increment_action)
        content.pack_start(duplicate_action_a, False, False, 0)
        duplicate_action_b = Gtk.Button.new_with_label("Duplicate action")
        duplicate_action_b.connect("clicked", self.increment_action)
        content.pack_start(duplicate_action_b, False, False, 0)

        enabled = Gtk.CheckButton.new_with_label("Enable feature")
        content.pack_start(enabled, False, False, 0)

        password = Gtk.Entry()
        password.set_placeholder_text("Senha")
        password.set_visibility(False)
        password.get_accessible().set_name("Senha")
        password.set_text("fixture-secret-value")
        content.pack_start(password, False, False, 0)

        # This is deliberately a plain text entry: its label, not a password
        # widget state, must still keep an access token out of model context.
        access_token = Gtk.Entry()
        access_token.set_placeholder_text("Token de acesso")
        access_token.get_accessible().set_name("Token de acesso")
        access_token.set_text("fixture-access-token-value")
        content.pack_start(access_token, False, False, 0)

        open_dialog = Gtk.Button.new_with_label("Open confirmation")
        open_dialog.connect("clicked", self.show_dialog)
        content.pack_start(open_dialog, False, False, 0)

        recreate = Gtk.Button.new_with_label("Recreate result")
        recreate.connect("clicked", self.recreate_result)
        content.pack_start(recreate, False, False, 0)

        self.list_box = Gtk.ListBox()
        self.list_box.set_selection_mode(Gtk.SelectionMode.NONE)
        self.list_box.get_accessible().set_name("Virtualized results (page 1 of 2)")
        content.pack_start(self.list_box, True, True, 0)
        self.add_list_row("Visible result")
        self.add_ephemeral_action(0)

        self.window.show_all()
        GLib.idle_add(self.mark_ready)

    def mark_ready(self) -> bool:
        self.ready_file.write_text("ready\n", encoding="utf-8")
        return False

    def increment_action(self, _button: Gtk.Button) -> None:
        self.counter += 1
        self.status.set_text(f"Action count: {self.counter}")

    def add_list_row(self, text: str) -> None:
        row = Gtk.ListBoxRow()
        label = Gtk.Label(label=text)
        label.set_xalign(0)
        row.add(label)
        self.list_box.add(row)
        row.show_all()

    def add_ephemeral_action(self, generation: int) -> None:
        row = Gtk.ListBoxRow()
        button = Gtk.Button.new_with_label(f"Ephemeral action {generation}")
        button.connect("clicked", self.increment_action)
        row.add(button)
        self.list_box.add(row)
        row.show_all()

    def recreate_result(self, _button: Gtk.Button) -> None:
        for child in self.list_box.get_children():
            self.list_box.remove(child)
        self.recreated += 1
        self.add_list_row(f"Recreated result {self.recreated}")
        self.add_ephemeral_action(self.recreated)
        self.list_box.get_accessible().set_name(
            f"Virtualized results (page {self.recreated + 1} of 2)"
        )

    def show_dialog(self, _button: Gtk.Button) -> None:
        self.dialog_status.set_text("Dialog state: open")
        dialog = Gtk.Dialog(title="Confirm action", transient_for=self.window, modal=True)
        dialog.get_accessible().set_name("Confirm action")
        label = Gtk.Label(label="Confirm action")
        label.set_xalign(0)
        dialog.get_content_area().add(label)
        dialog.add_button("Cancel", Gtk.ResponseType.CANCEL)
        dialog.add_button("Confirm", Gtk.ResponseType.OK)
        dialog.connect("response", self.close_dialog)
        dialog.show_all()

    def close_dialog(self, dialog: Gtk.Dialog, response: int) -> None:
        if response == Gtk.ResponseType.OK:
            self.dialog_status.set_text("Dialog state: confirmed")
        else:
            self.dialog_status.set_text("Dialog state: cancelled")
        dialog.destroy()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ready-file", type=pathlib.Path, required=True)
    args = parser.parse_args()
    args.ready_file.unlink(missing_ok=True)

    def stop(_signal: int, _frame: object) -> None:
        Gtk.main_quit()

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    AccessibilityFixture(args.ready_file)
    Gtk.main()


if __name__ == "__main__":
    main()
