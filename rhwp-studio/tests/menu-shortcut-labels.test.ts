import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { CommandRegistry } from '../src/command/registry.ts';
import { syncMenuShortcutLabels } from '../src/ui/menu-shortcut-labels.ts';
import type { CommandDef } from '../src/command/types.ts';
import type { PlatformKind } from '../src/engine/navigation-keymap.ts';

type TestGlobal = typeof globalThis & {
  __rhwpTestPlatformKind?: PlatformKind;
  document?: unknown;
};

const rootDir = dirname(dirname(fileURLToPath(import.meta.url)));

function source(path: string): string {
  return readFileSync(join(rootDir, path), 'utf8');
}

function assertCommandShortcut(src: string, commandId: string, shortcutLabel: string): void {
  const start = src.indexOf(`id: '${commandId}'`);
  assert.notEqual(start, -1, `${commandId} command not found`);
  const next = src.indexOf('\n  {', start + 1);
  const block = src.slice(start, next === -1 ? undefined : next);
  assert.match(block, new RegExp(`shortcutLabel:\\s*'${shortcutLabel.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}'`));
}

class FakeSpan {
  className = '';
  textContent = '';
}

class FakeMenuItem {
  dataset: { cmd?: string };
  shortcut: FakeSpan | null;

  constructor(cmd: string, shortcutText?: string) {
    this.dataset = { cmd };
    this.shortcut = shortcutText === undefined ? null : new FakeSpan();
    if (this.shortcut) this.shortcut.textContent = shortcutText;
  }

  querySelector(selector: string): FakeSpan | null {
    if (selector === '.md-shortcut') return this.shortcut;
    return null;
  }

  appendChild(child: FakeSpan): FakeSpan {
    this.shortcut = child;
    return child;
  }
}

class FakeContainer {
  readonly items: FakeMenuItem[];

  constructor(items: FakeMenuItem[]) {
    this.items = items;
  }

  querySelectorAll(selector: string): FakeMenuItem[] {
    assert.equal(selector, '.md-item[data-cmd]');
    return this.items;
  }
}

function registryWith(command: Pick<CommandDef, 'id' | 'shortcutLabel'>): CommandRegistry {
  const registry = new CommandRegistry();
  registry.register({
    id: command.id,
    label: command.id,
    shortcutLabel: command.shortcutLabel,
    execute: () => {},
  });
  return registry;
}

function withPlatform(platform: PlatformKind, run: () => void): void {
  const globalForTest = globalThis as TestGlobal;
  const previousPlatform = globalForTest.__rhwpTestPlatformKind;
  const previousDocument = globalForTest.document;
  globalForTest.__rhwpTestPlatformKind = platform;
  globalForTest.document = {
    createElement(tagName: string) {
      assert.equal(tagName, 'span');
      return new FakeSpan();
    },
  };
  try {
    run();
  } finally {
    if (previousPlatform === undefined) {
      delete globalForTest.__rhwpTestPlatformKind;
    } else {
      globalForTest.__rhwpTestPlatformKind = previousPlatform;
    }
    if (previousDocument === undefined) {
      delete globalForTest.document;
    } else {
      globalForTest.document = previousDocument;
    }
  }
}

test('상단 메뉴 단축키는 macOS에서 ⌘ 기호로 변환된다', () => {
  withPlatform('mac', () => {
    const item = new FakeMenuItem('file:save', 'Ctrl+S');
    syncMenuShortcutLabels(
      new FakeContainer([item]) as unknown as HTMLElement,
      registryWith({ id: 'file:save', shortcutLabel: 'Ctrl+S' }),
    );

    assert.equal(item.shortcut?.textContent, '⌘S');
  });
});

test('상단 메뉴 단축키는 macOS에서 ⌥ 기호로 변환된다', () => {
  withPlatform('mac', () => {
    const item = new FakeMenuItem('file:new-doc', 'Alt+N');
    syncMenuShortcutLabels(
      new FakeContainer([item]) as unknown as HTMLElement,
      registryWith({ id: 'file:new-doc', shortcutLabel: 'Alt+N' }),
    );

    assert.equal(item.shortcut?.textContent, '⌥N');
  });
});

test('상단 메뉴 단축키는 Windows/Linux 계열에서 Ctrl 표기를 유지한다', () => {
  withPlatform('other', () => {
    const item = new FakeMenuItem('file:save', 'Ctrl+S');
    syncMenuShortcutLabels(
      new FakeContainer([item]) as unknown as HTMLElement,
      registryWith({ id: 'file:save', shortcutLabel: 'Ctrl+S' }),
    );

    assert.equal(item.shortcut?.textContent, 'Ctrl+S');
  });
});

test('상단 메뉴 단축키는 하드코딩 값보다 CommandRegistry 정의를 우선한다', () => {
  withPlatform('mac', () => {
    const item = new FakeMenuItem('file:save-as', 'Ctrl+OLD');
    syncMenuShortcutLabels(
      new FakeContainer([item]) as unknown as HTMLElement,
      registryWith({ id: 'file:save-as', shortcutLabel: 'Ctrl+Shift+S' }),
    );

    assert.equal(item.shortcut?.textContent, '⌘⇧S');
  });
});

test('상단 메뉴 항목에 md-shortcut이 없으면 registry 값을 기준으로 생성한다', () => {
  withPlatform('mac', () => {
    const item = new FakeMenuItem('file:print');
    syncMenuShortcutLabels(
      new FakeContainer([item]) as unknown as HTMLElement,
      registryWith({ id: 'file:print', shortcutLabel: 'Ctrl+P' }),
    );

    assert.equal(item.shortcut?.className, 'md-shortcut');
    assert.equal(item.shortcut?.textContent, '⌘P');
  });
});

test('상단 메뉴 하드코딩 단축키와 registry shortcutLabel의 누락 항목을 고정한다', () => {
  const view = source('src/command/commands/view.ts');
  const format = source('src/command/commands/format.ts');

  assertCommandShortcut(view, 'view:zoom-fit-page', 'Ctrl+G,P');
  assertCommandShortcut(view, 'view:zoom-fit-width', 'Ctrl+G,W');
  assert.match(view, /zoomLevel\(100,\s*'Ctrl\+G,Q'\)/);
  assertCommandShortcut(view, 'view:para-mark', 'Ctrl+G,T');
  assertCommandShortcut(view, 'view:border-transparent', 'Alt+V,T');

  assertCommandShortcut(format, 'format:font-size-increase', 'Alt+Shift+E');
  assertCommandShortcut(format, 'format:font-size-decrease', 'Alt+Shift+R');
  assertCommandShortcut(format, 'format:align-left', 'Ctrl+Shift+L');
  assertCommandShortcut(format, 'format:align-center', 'Alt+Shift+C');
  assertCommandShortcut(format, 'format:align-right', 'Alt+Shift+H');
  assertCommandShortcut(format, 'format:align-justify', 'Ctrl+Shift+M');
  assertCommandShortcut(format, 'format:align-distribute', 'Alt+Shift+D');
  assertCommandShortcut(format, 'format:line-spacing-increase', 'Alt+Shift+Z');
  assertCommandShortcut(format, 'format:line-spacing-decrease', 'Alt+Shift+A');
});
