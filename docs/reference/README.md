# Reference sources

`docs/es9-sysex-protocol.md` was read from the material listed below. None of it is
redistributed here: the manual is Expert Sleepers' copyrighted documentation, and the
configuration tool is better fetched from its own repository than pinned as a stale copy.

Everything in this directory apart from this file is ignored by git, so a local working
copy can sit here without being published.

| Expected filename | What it is | Where to get it |
|---|---|---|
| `es9_user_manual_1.3.txt` | Official manual. Its SysEx appendix (pages 15–19) is the specification. | [expert-sleepers.co.uk](https://expert-sleepers.co.uk/es9.html) — download the v1.3 manual PDF and extract the text |
| `es9_config_tool_official_fw1.3.html` | The official configuration tool, firmware 1.3.0. MIT licensed, Copyright (c) 2023 Expert Sleepers Ltd. | [expertsleepersltd/ES-9_tools](https://github.com/expertsleepersltd/ES-9_tools) |
| `es9_config_tool_1.2.html` | The older tool, firmware 1.2. Worth keeping because the config dump format differs. | Same repository, earlier history |

The protocol document stands on its own — nothing in the build or the tests reads these
files. They are needed only to check a claim back against its source, or to work on a
part of the protocol this project does not implement yet.
