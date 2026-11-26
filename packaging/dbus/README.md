# Desktop registration

Installed by packaging (item 12):

- `asc_files.portal` → `/usr/share/xdg-desktop-portal/portals/`: Files offers the FileChooser.
  A desktop chooses it with `org.freedesktop.impl.portal.FileChooser=asc_files` in its
  `portals.conf` (Settings › Integrations will write that, as legacy's did).
- `org.freedesktop.impl.portal.desktop.asc_files.service` → `/usr/share/dbus-1/services/`: the
  portal starts `asc-files --chooser`, which ends once idle.
- `org.freedesktop.FileManager1.service` → `/usr/share/dbus-1/services/`: "Show in folder" starts
  Files when it isn't running.
