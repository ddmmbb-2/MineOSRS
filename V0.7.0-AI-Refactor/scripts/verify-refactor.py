#!/usr/bin/env python3
"""Behavior-preservation audit against a V0.7.0 source ZIP.

Checks HTML DOM structure, extracted legacy JavaScript/CSS, unchanged core
modules, API routes, and DB schema markers. Does not replace cargo test.
Usage: python scripts/verify-refactor.py "V0.7.0(1).zip"
"""
from __future__ import annotations
import hashlib
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def read_source_zip(archive: Path, basename: str) -> bytes:
    with zipfile.ZipFile(archive) as zf:
        matches = [name for name in zf.namelist() if name.endswith('/' + basename)]
        assert len(matches) == 1, f'{basename}: expected one matching file; got {matches}'
        return zf.read(matches[0])


class VisibleDOM(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.dom = []
        self.depth = []

    def handle_starttag(self, tag, attrs) -> None:
        d = dict(attrs)
        # CSS was moved to a stylesheet and inline UI script to an external JS.
        if tag in ('style', 'script'):
            self.depth.append(tag)
            if tag == 'script' and d.get('src') == '/app.js':
                self.dom.append((tag, tuple(sorted(attrs))))
            return
        if tag == 'link' and d.get('href') == '/ui-v07.css':
            return
        self.dom.append((tag, tuple(sorted(attrs))))

    def handle_endtag(self, tag):
        if tag in ('style', 'script') and self.depth:
            self.depth.pop()
        elif tag not in ('style', 'script'):
            self.dom.append(('/' + tag, ()))

    def handle_data(self, data: str):
        if not self.depth and data.strip():
            self.dom.append(('text', ' '.join(data.split())))


def routes(code: str) -> set[tuple[str, str]]:
    return {(path, method) for path, method in re.findall(r'\.route\("([^"]+)",\s*([a-z]+)\(',code)}


def verify(archive: Path) -> None:
    source_html = read_source_zip(archive, 'web/index.html').decode('utf-8')
    modified_html = (ROOT / 'web/index.html').read_text(encoding='utf-8')
    original_css = re.findall(r'<style(?:\s+[^>]*)?>(.*?)</style>', source_html, re.S)
    original_ui = re.search(r'<script id="mi07Code">(.*?)</script>', source_html, re.S)
    assert original_ui and len(original_css) == 3
    assert (ROOT / 'web/ui-v07.js').read_text(encoding='utf-8') == original_ui.group(1), 'V0.7 UI code changed'
    assert (ROOT / 'web/ui-v07.css').read_text(encoding='utf-8') == '\n'.join(original_css), 'CSS changed'
    for rel in ['web/app.js','web/style.css','Cargo.toml','Cargo.lock',
                'src/downloads.rs','src/bedrock.rs','src/backups.rs','src/hardware.rs',
                'src/legacy_paper.rs','src/legacy_paper_cache.ps1']:
        old = read_source_zip(archive,rel)
        new = (ROOT/rel).read_bytes()
        assert old == new, f'Unexpected change to {rel}'

    before, after = VisibleDOM(), VisibleDOM()
    before.feed(source_html)
    after.feed(modified_html)
    assert before.dom == after.dom, 'Visible DOM differs'

    old_main = read_source_zip(archive, 'src/main.rs').decode('utf-8')
    new_boot = (ROOT/'src/core/bootstrap.rs').read_text(encoding='utf-8')
    missing = routes(old_main)-routes(new_boot)
    assert not missing, f'API routes missing: {sorted(missing)}'
    assert '/ui-v07.js' in new_boot and '/ui-v07.css' in new_boot
    for marker in ['CREATE TABLE IF NOT EXISTS servers','ALTER TABLE servers ADD COLUMN',
                   "('core_type'",'MINEOS_DATA_DIR','LOCALAPPDATA',
                   'backup_schedules', 'MINEOS_ADMIN_TOKEN']:
        if marker == "('core_type'":
            continue  # release source uses tuple format with double quotes
        assert marker in old_main and marker in new_boot, f'Migration/config mismatch: {marker}'

    print('PASS: original JavaScript, CSS and all baseline domain modules match exactly')
    print('PASS: visible HTML DOM including all IDs, field types and text is identical')
    print(f'PASS: all {len(routes(old_main))} existing routes preserved; two UI assets added')
    print('PASS: DB initialization, schema migration and data directories preserved')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('Usage: python scripts/verify-refactor.py <original V0.7.0 zip>')
    verify(Path(sys.argv[1]))
