# python-hwpx external fixture

This directory contains small public HWPX fixtures used only for targeted
MCP compatibility tests.

Source:
- Repository: https://github.com/airmang/python-hwpx
- Cloned commit used for import: `8f9b8cd` (`2026-07-02`)
- Original hidden-comment path:
  `tests/fixtures/hwpxlib_corpus/error__20230413__test.hwpx`
- Original inline track-change path:
  `shared/hwpx/fixtures/change-tracking/40_trackchange_min.hwpx`
- Original insert/delete body track-change path:
  `tests/fixtures/hwpxlib_corpus/reader_writer__ChangeTrack.hwpx`
- Original project license: Apache-2.0
- Original notice: see `NOTICE.python-hwpx`

The source project NOTICE states that `tests/fixtures/hwpxlib_corpus/` is
vendored from `neolord0/hwpxlib`, licensed Apache-2.0. The copied fixture is
used because the local `samples/` corpus did not contain an authored
`hp:hiddenComment` HWPX body candidate. The MCP representative smoke opens this
file, edits the hidden-comment body, saves it as HWPX, reopens it, and verifies
that the hidden-comment control and edited text survive. The inline track-change
fixture is an editor-authored sanitized baseline from the source project; its
`Contents/header.xml` contains `hh:trackChanges` and `hh:trackChangeAuthors`
inside `hh:refList`, which is distinct from TrackChange/Revisions/History
auxiliary package entries. `reader_writer__ChangeTrack.hwpx` adds the same
public fixture family's `hp:deleteBegin`/`hp:deleteEnd` body range markers so
the MCP regression covers both inserted and deleted body ranges without adding a
large same-type fixture sweep.
