---
name: status
description: Check whether lucid-dream is connected in this session and its memory server answers. Run it when shared memory looks missing or after installing or upgrading lucid-dream.
disable-model-invocation: true
---

# lucid-dream status

Report on the lucid-dream install this session uses. Only report: do not
change any file, setting or hook, and run no command other than the two
below.

1. Run `lucid-dream --version`. If the command is not found, say that the
   lucid-dream program is not installed (or not on `PATH`), so this session
   has no shared memory, and point to
   <https://github.com/jksim26/lucid-dream#install>. Stop there.
2. Run `lucid-dream doctor`.

Summarise the output in a few short lines: whether the server answered and
at which address, whether the hooks are installed and by which route (plugin
or settings file), the installed version against the latest release, and
each warning or failure with what it says to do about it.
