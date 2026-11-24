#!/bin/sh
# Let the desktop know about .purr files and their icon.
update-mime-database /usr/share/mime >/dev/null 2>&1 || true
gtk-update-icon-cache -q -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
exit 0
