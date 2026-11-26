# Desktop registration

Installed by `cmake --install` (and the RPM):

- `asc_files.portal` → `/usr/share/xdg-desktop-portal/portals/`: Files offers the FileChooser.
  A desktop chooses it with `org.freedesktop.impl.portal.FileChooser=asc_files` in its
  `portals.conf` (Settings › Integrations writes that, as legacy's did).
- `org.freedesktop.impl.portal.desktop.asc_files.service` (from the `.in` here, with the install
  prefix) → `/usr/share/dbus-1/services/`: the portal starts `asc-files --chooser`, which ends once idle.

Not installed system-wide:

- `org.freedesktop.FileManager1.service`: "Show in folder" starting Files. Another file manager
  (Nautilus) ships one for the same name, so making Files the default is the user's choice:
  Settings › Integrations › Default file manager writes it under `~/.local/share/dbus-1/services/`
  and sets Files as the folder handler. This copy is kept for distributions that ship Files as
  their only file manager.
