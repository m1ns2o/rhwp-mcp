import type { WasmBridge } from '@/core/wasm-bridge';
import type { DocumentPosition } from '@/core/types';
import type { InputHandler } from '@/engine/input-handler';
import {
  DeleteSelectionCommand,
  InsertTextCommand,
  SplitParagraphCommand,
  SplitParagraphInCellCommand,
} from '@/engine/command';

export type AiProvider = 'openai' | 'anthropic' | 'gemini' | 'custom';
export type AiAuthMode = 'apiKey' | 'bearer';

export interface AiChatSettingsInput {
  provider?: AiProvider;
  authMode?: AiAuthMode;
  model?: string;
  apiKey?: string;
  bearerToken?: string;
  customEndpoint?: string;
  oauthEndpoint?: string;
  oauthClientId?: string;
  oauthScope?: string;
  maxContextChars?: number;
}

export interface AiChatPublicSettings {
  provider: AiProvider;
  authMode: AiAuthMode;
  model: string;
  customEndpoint: string;
  oauthEndpoint: string;
  oauthClientId: string;
  oauthScope: string;
  maxContextChars: number;
  hasApiKey: boolean;
  hasBearerToken: boolean;
}

interface AiChatSettings {
  provider: AiProvider;
  authMode: AiAuthMode;
  model: string;
  apiKey: string;
  bearerToken: string;
  customEndpoint: string;
  oauthEndpoint: string;
  oauthClientId: string;
  oauthScope: string;
  maxContextChars: number;
}

interface AiChatPanelOptions {
  wasm: WasmBridge;
  getInputHandler: () => InputHandler | null;
  getCurrentPage: () => number;
  onRequestClose?: () => void;
}

interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  text: string;
}

type AiEditorAction =
  | { type: 'insert_text'; text: string }
  | { type: 'replace_selection'; text: string }
  | { type: 'create_table'; rows: number; cols: number; cells?: string[][]; treatAsChar?: boolean };

interface AiEditorActionPayload {
  message?: string;
  rhwpActions?: unknown;
}

interface RhwpDesktopBridge {
  geminiGenerate?: (payload: {
    model: string;
    input: string;
    apiKey?: string;
    bearerToken?: string;
  }) => Promise<unknown>;
  mcpRequest?: (method: string, params?: unknown) => Promise<unknown>;
  platform?: string;
}

declare global {
  interface Window {
    rhwpDesktop?: RhwpDesktopBridge;
  }
}

const STORAGE_KEY = 'rhwp.aiChat.settings.v1';
const WIDTH_STORAGE_KEY = 'rhwp.aiChat.width.v1';
const MIN_PANEL_WIDTH = 340;
const MAX_PANEL_WIDTH = 560;
const DEFAULT_SETTINGS: AiChatSettings = {
  provider: 'openai',
  authMode: 'apiKey',
  model: 'gpt-5.5',
  apiKey: '',
  bearerToken: '',
  customEndpoint: '',
  oauthEndpoint: '',
  oauthClientId: '',
  oauthScope: '',
  maxContextChars: 6000,
};

const PROVIDER_DEFAULT_MODELS: Record<AiProvider, string> = {
  openai: 'gpt-5.5',
  anthropic: 'claude-sonnet-5',
  gemini: 'gemini-flash-latest',
  custom: '',
};

const PROVIDER_LABELS: Record<AiProvider, string> = {
  openai: 'OpenAI',
  anthropic: 'Claude',
  gemini: 'Gemini',
  custom: 'Custom',
};

export class AiChatPanel {
  private settings: AiChatSettings = loadSettings();
  private messages: ChatMessage[] = [];
  private sending = false;
  private connecting = false;

  private providerEl!: HTMLSelectElement;
  private authModeEl!: HTMLSelectElement;
  private modelEl!: HTMLInputElement;
  private apiKeyEl!: HTMLInputElement;
  private bearerTokenEl!: HTMLInputElement;
  private customEndpointEl!: HTMLInputElement;
  private oauthEndpointEl!: HTMLInputElement;
  private oauthClientIdEl!: HTMLInputElement;
  private oauthScopeEl!: HTMLInputElement;
  private maxContextEl!: HTMLInputElement;
  private messagesEl!: HTMLDivElement;
  private promptEl!: HTMLTextAreaElement;
  private sendBtn!: HTMLButtonElement;
  private statusEl!: HTMLSpanElement;
  private docStateEl!: HTMLSpanElement;
  private connectionSummaryEl!: HTMLDivElement;
  private connectionDetailEl!: HTMLDivElement;
  private emptyStateEl!: HTMLDivElement;
  private saveSettingsBtn!: HTMLButtonElement;
  private testConnectionBtn!: HTMLButtonElement;
  private refreshTokenBtn!: HTMLButtonElement;
  private settingsBodyEl!: HTMLDivElement;
  private resizeHandleEl!: HTMLDivElement;

  constructor(private root: HTMLElement, private options: AiChatPanelOptions) {
    this.initializePanelWidth();
    this.render();
    this.bind();
    this.applySettingsToControls();
    this.refreshDocumentState();
  }

  refreshDocumentState(): void {
    const { wasm } = this.options;
    if (!this.docStateEl) return;
    if (wasm.pageCount <= 0) {
      this.docStateEl.textContent = '문서 없음';
      this.sendBtn.disabled = this.sending;
      return;
    }
    const currentPage = Math.min(this.options.getCurrentPage(), Math.max(0, wasm.pageCount - 1));
    this.docStateEl.textContent = `${wasm.fileName} · ${currentPage + 1}/${wasm.pageCount}`;
    this.sendBtn.disabled = this.sending;
  }

  configure(input: Record<string, unknown>): AiChatPublicSettings {
    const provider = isProvider(input.provider) ? input.provider : this.settings.provider;
    const authMode = isAuthMode(input.authMode) ? input.authMode : this.settings.authMode;
    const providerChanged = provider !== this.settings.provider;
    const modelFallback = providerChanged ? PROVIDER_DEFAULT_MODELS[provider] : this.settings.model;
    this.settings = {
      ...this.settings,
      provider,
      authMode,
      model: stringSetting(input.model, modelFallback || PROVIDER_DEFAULT_MODELS[provider]),
      apiKey: stringSetting(input.apiKey, this.settings.apiKey),
      bearerToken: stringSetting(input.bearerToken, this.settings.bearerToken),
      customEndpoint: stringSetting(input.customEndpoint, this.settings.customEndpoint),
      oauthEndpoint: stringSetting(input.oauthEndpoint, this.settings.oauthEndpoint),
      oauthClientId: stringSetting(input.oauthClientId, this.settings.oauthClientId),
      oauthScope: stringSetting(input.oauthScope, this.settings.oauthScope),
      maxContextChars: input.maxContextChars === undefined
        ? this.settings.maxContextChars
        : clampInt(Number(input.maxContextChars), 1000, 24000, this.settings.maxContextChars),
    };
    if (!this.settings.model) {
      this.settings.model = PROVIDER_DEFAULT_MODELS[provider];
    }
    saveSettings(this.settings);
    this.applySettingsToControls();
    this.refreshDocumentState();
    return this.publicSettings();
  }

  publicSettings(): AiChatPublicSettings {
    return {
      provider: this.settings.provider,
      authMode: this.settings.authMode,
      model: this.settings.model,
      customEndpoint: this.settings.customEndpoint,
      oauthEndpoint: this.settings.oauthEndpoint,
      oauthClientId: this.settings.oauthClientId,
      oauthScope: this.settings.oauthScope,
      maxContextChars: this.settings.maxContextChars,
      hasApiKey: this.settings.apiKey.length > 0,
      hasBearerToken: this.settings.bearerToken.length > 0,
    };
  }

  private render(): void {
    this.root.innerHTML = `
      <div class="ai-chat-resize-handle" data-role="resize-handle" title="사이드바 너비 조정" aria-hidden="true"></div>
      <div class="ai-chat-header">
        <div class="ai-chat-title">
          <strong>AI Assistant</strong>
          <span data-role="doc-state">문서 없음</span>
        </div>
        <div class="ai-chat-header-actions">
          <button type="button" class="ai-chat-icon-btn" data-role="toggle-settings" title="AI 설정" aria-label="AI 설정">⚙</button>
          <button type="button" class="ai-chat-icon-btn" data-role="close-panel" title="AI 사이드바 닫기" aria-label="AI 사이드바 닫기">×</button>
        </div>
      </div>
      <div class="ai-chat-settings" data-role="settings">
        <div class="ai-chat-connection">
          <div class="ai-chat-connection-main">
            <span class="ai-chat-connection-dot" aria-hidden="true"></span>
            <div>
              <div class="ai-chat-connection-summary" data-role="connection-summary"></div>
              <div class="ai-chat-connection-detail" data-role="connection-detail">설정 후 연결 테스트를 실행하세요.</div>
            </div>
          </div>
          <button type="button" data-role="test-connection">테스트</button>
        </div>
        <div class="ai-chat-controls">
          <label class="ai-chat-field">
            <span>Provider</span>
            <select data-role="provider">
              <option value="openai">OpenAI</option>
              <option value="anthropic">Claude</option>
              <option value="gemini">Gemini</option>
              <option value="custom">Custom</option>
            </select>
          </label>
          <label class="ai-chat-field">
            <span>Model</span>
            <input data-role="model" autocomplete="off" />
          </label>
        </div>
        <select data-role="auth-mode" class="ai-chat-hidden-select" aria-hidden="true" tabindex="-1">
          <option value="apiKey">API key</option>
          <option value="bearer">OAuth</option>
        </select>
        <div class="ai-auth-toggle" role="group" aria-label="AI 인증 방식">
          <button type="button" data-auth-choice="apiKey">API key</button>
          <button type="button" data-auth-choice="bearer">OAuth</button>
        </div>
        <label class="ai-chat-field" data-auth-panel="apiKey">
          <span>API key</span>
          <input data-role="api-key" type="password" autocomplete="off" placeholder="키 입력 후 테스트" />
        </label>
        <label class="ai-chat-field" data-provider-panel="custom">
          <span>Endpoint</span>
          <input data-role="custom-endpoint" type="url" autocomplete="off" />
        </label>
        <div class="ai-chat-oauth" data-auth-panel="bearer">
          <label class="ai-chat-field">
            <span>Token broker URL</span>
            <input data-role="oauth-endpoint" type="url" autocomplete="off" placeholder="/api/ai-token" />
          </label>
          <details class="ai-chat-advanced">
            <summary>OAuth 옵션</summary>
            <label class="ai-chat-field">
              <span>Client ID</span>
              <input data-role="oauth-client-id" autocomplete="off" />
            </label>
            <label class="ai-chat-field">
              <span>Scope</span>
              <input data-role="oauth-scope" autocomplete="off" />
            </label>
            <label class="ai-chat-field">
              <span>Bearer token</span>
              <input data-role="bearer-token" type="password" autocomplete="off" placeholder="token broker 사용 시 자동 저장" />
            </label>
          </details>
        </div>
        <details class="ai-chat-advanced">
          <summary>고급</summary>
          <label class="ai-chat-field">
            <span>Context chars</span>
            <input data-role="max-context" type="number" min="1000" max="24000" step="500" />
          </label>
        </details>
        <div class="ai-chat-settings-actions">
          <button type="button" data-role="save-settings">저장</button>
          <button type="button" data-role="refresh-token">OAuth 로그인</button>
        </div>
      </div>
      <div class="ai-chat-messages" data-role="messages" aria-live="polite">
        <div class="ai-chat-empty" data-role="empty-state">
          <strong>문서와 대화</strong>
          <span>문서를 열고 질문하거나, 먼저 provider 연결을 테스트하세요.</span>
        </div>
      </div>
      <form class="ai-chat-compose" data-role="form">
        <span data-role="status" class="ai-chat-status"></span>
        <div class="ai-chat-composer-box">
          <textarea data-role="prompt" rows="1" placeholder="메시지를 입력하세요"></textarea>
          <button type="submit" data-role="send" title="보내기" aria-label="보내기">
            <span aria-hidden="true">↑</span>
          </button>
        </div>
      </form>
    `;

    this.providerEl = this.query('provider', HTMLSelectElement);
    this.authModeEl = this.query('auth-mode', HTMLSelectElement);
    this.modelEl = this.query('model', HTMLInputElement);
    this.apiKeyEl = this.query('api-key', HTMLInputElement);
    this.bearerTokenEl = this.query('bearer-token', HTMLInputElement);
    this.customEndpointEl = this.query('custom-endpoint', HTMLInputElement);
    this.oauthEndpointEl = this.query('oauth-endpoint', HTMLInputElement);
    this.oauthClientIdEl = this.query('oauth-client-id', HTMLInputElement);
    this.oauthScopeEl = this.query('oauth-scope', HTMLInputElement);
    this.maxContextEl = this.query('max-context', HTMLInputElement);
    this.messagesEl = this.query('messages', HTMLDivElement);
    this.promptEl = this.query('prompt', HTMLTextAreaElement);
    this.sendBtn = this.query('send', HTMLButtonElement);
    this.statusEl = this.query('status', HTMLSpanElement);
    this.docStateEl = this.query('doc-state', HTMLSpanElement);
    this.connectionSummaryEl = this.query('connection-summary', HTMLDivElement);
    this.connectionDetailEl = this.query('connection-detail', HTMLDivElement);
    this.emptyStateEl = this.query('empty-state', HTMLDivElement);
    this.saveSettingsBtn = this.query('save-settings', HTMLButtonElement);
    this.testConnectionBtn = this.query('test-connection', HTMLButtonElement);
    this.refreshTokenBtn = this.query('refresh-token', HTMLButtonElement);
    this.settingsBodyEl = this.query('settings', HTMLDivElement);
    this.resizeHandleEl = this.query('resize-handle', HTMLDivElement);
  }

  private bind(): void {
    this.root.querySelector('[data-role="form"]')?.addEventListener('submit', (event) => {
      event.preventDefault();
      void this.sendPrompt();
    });
    this.root.querySelector('[data-role="toggle-settings"]')?.addEventListener('click', () => {
      this.settingsBodyEl.classList.toggle('collapsed');
    });
    this.root.querySelector('[data-role="close-panel"]')?.addEventListener('click', () => {
      this.options.onRequestClose?.();
    });
    this.bindResizeHandle();
    this.saveSettingsBtn.addEventListener('click', () => {
      this.readSettingsFromControls();
      saveSettings(this.settings);
      this.setStatus('설정 저장됨');
      this.setConnectionState('saved', '저장됨. 연결 테스트로 실제 호출 가능 여부를 확인하세요.');
      this.settingsBodyEl.classList.add('collapsed');
    });
    this.testConnectionBtn.addEventListener('click', () => {
      void this.testConnection().catch(() => undefined);
    });
    this.refreshTokenBtn.addEventListener('click', () => {
      void this.refreshOAuthToken().catch(() => undefined);
    });
    this.root.querySelectorAll<HTMLButtonElement>('[data-auth-choice]').forEach((button) => {
      button.addEventListener('click', () => {
        const authMode = button.dataset.authChoice;
        if (!isAuthMode(authMode)) return;
        this.authModeEl.value = authMode;
        this.readSettingsFromControls();
        this.updateVisibleSettings();
        this.setConnectionState('idle', authMode === 'apiKey'
          ? 'API key를 저장한 뒤 연결 테스트를 실행하세요.'
          : 'Token broker URL을 입력한 뒤 OAuth 로그인을 실행하세요.');
      });
    });
    this.providerEl.addEventListener('change', () => {
      const provider = this.providerEl.value as AiProvider;
      const previousProvider = this.settings.provider;
      const currentModel = this.modelEl.value.trim();
      if (!currentModel || currentModel === PROVIDER_DEFAULT_MODELS[previousProvider]) {
        this.modelEl.value = PROVIDER_DEFAULT_MODELS[provider];
      }
      this.readSettingsFromControls();
      saveSettings(this.settings);
      this.applySettingsToControls();
      this.updateVisibleSettings();
      this.setConnectionState('idle', `${PROVIDER_LABELS[provider]} 설정을 확인하세요.`);
    });
    this.authModeEl.addEventListener('change', () => {
      this.readSettingsFromControls();
      this.updateVisibleSettings();
      this.updateConnectionSummary();
    });
    this.promptEl.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
        event.preventDefault();
        void this.sendPrompt();
      }
    });
    this.promptEl.addEventListener('input', () => this.resizePromptInput());
    this.resizePromptInput();
  }

  private applySettingsToControls(): void {
    this.providerEl.value = this.settings.provider;
    this.authModeEl.value = this.settings.authMode;
    this.modelEl.value = this.settings.model;
    this.apiKeyEl.value = this.settings.apiKey;
    this.bearerTokenEl.value = this.settings.bearerToken;
    this.customEndpointEl.value = this.settings.customEndpoint;
    this.oauthEndpointEl.value = this.settings.oauthEndpoint;
    this.oauthClientIdEl.value = this.settings.oauthClientId;
    this.oauthScopeEl.value = this.settings.oauthScope;
    this.maxContextEl.value = String(this.settings.maxContextChars);
    this.updateVisibleSettings();
    this.updateConnectionSummary();
    this.syncAuthButtons();
  }

  private readSettingsFromControls(): void {
    const provider = this.providerEl.value as AiProvider;
    const authMode = this.authModeEl.value as AiAuthMode;
    this.settings = {
      provider,
      authMode,
      model: this.modelEl.value.trim() || PROVIDER_DEFAULT_MODELS[provider],
      apiKey: this.apiKeyEl.value.trim(),
      bearerToken: this.bearerTokenEl.value.trim(),
      customEndpoint: this.customEndpointEl.value.trim(),
      oauthEndpoint: this.oauthEndpointEl.value.trim(),
      oauthClientId: this.oauthClientIdEl.value.trim(),
      oauthScope: this.oauthScopeEl.value.trim(),
      maxContextChars: clampInt(Number(this.maxContextEl.value), 1000, 24000, 6000),
    };
    this.maxContextEl.value = String(this.settings.maxContextChars);
    this.updateConnectionSummary();
  }

  private updateVisibleSettings(): void {
    const provider = this.providerEl.value as AiProvider;
    const authMode = this.authModeEl.value as AiAuthMode;
    this.root.querySelectorAll<HTMLElement>('[data-provider-panel]').forEach((element) => {
      element.hidden = element.dataset.providerPanel !== provider;
    });
    this.root.querySelectorAll<HTMLElement>('[data-auth-panel]').forEach((element) => {
      element.hidden = element.dataset.authPanel !== authMode;
    });
    const refreshButton = this.root.querySelector<HTMLButtonElement>('[data-role="refresh-token"]');
    if (refreshButton) refreshButton.hidden = authMode !== 'bearer';
    this.syncAuthButtons();
    this.updateConnectionSummary();
  }

  private updateConnectionSummary(): void {
    if (!this.connectionSummaryEl) return;
    const provider = this.providerEl?.value as AiProvider || this.settings.provider;
    const authMode = this.authModeEl?.value as AiAuthMode || this.settings.authMode;
    const hasCredential = authMode === 'apiKey'
      ? this.apiKeyEl?.value.trim().length > 0 || this.settings.apiKey.length > 0
      : this.bearerTokenEl?.value.trim().length > 0 || this.settings.bearerToken.length > 0;
    const authLabel = authMode === 'apiKey' ? 'API key' : 'OAuth';
    this.connectionSummaryEl.textContent = `${PROVIDER_LABELS[provider]} · ${authLabel} · ${hasCredential ? 'credential 저장됨' : '미설정'}`;
  }

  private syncAuthButtons(): void {
    const authMode = this.authModeEl.value as AiAuthMode;
    this.root.querySelectorAll<HTMLButtonElement>('[data-auth-choice]').forEach((button) => {
      button.classList.toggle('active', button.dataset.authChoice === authMode);
      button.setAttribute('aria-pressed', button.dataset.authChoice === authMode ? 'true' : 'false');
    });
  }

  private setConnectionState(state: 'idle' | 'saved' | 'testing' | 'connected' | 'failed', detail: string): void {
    this.root.dataset.connectionState = state;
    this.connectionDetailEl.textContent = detail;
    this.updateConnectionSummary();
  }

  private async sendPrompt(): Promise<void> {
    if (this.sending) return;
    const prompt = this.promptEl.value.trim();
    if (!prompt) return;

    this.readSettingsFromControls();
    saveSettings(this.settings);
    this.addMessage('user', prompt);
    this.promptEl.value = '';
    this.resizePromptInput();
    this.setSending(true);

    try {
      const documentContext = this.buildDocumentContext();
      const providerAnswer = await this.callProvider(prompt, documentContext);
      const actionResult = extractEditorActions(providerAnswer);
      const applied = this.applyEditorActions(actionResult.actions);
      const answer = composeAssistantMessage(actionResult.text, applied);
      this.addMessage('assistant', answer);
      this.setConnectionState('connected', '응답 수신 완료');
      this.setStatus('');
    } catch (error) {
      const message = friendlyAiError(error, this.settings);
      this.addMessage('assistant', `요청 실패: ${message}`);
      this.setConnectionState('failed', message);
      this.setStatus('실패');
    } finally {
      this.setSending(false);
    }
  }

  async refreshOAuthToken(): Promise<AiChatPublicSettings> {
    this.readSettingsFromControls();
    if (!this.settings.oauthEndpoint) {
      const message = 'Token broker URL을 입력하세요.';
      this.setStatus(message);
      this.setConnectionState('failed', message);
      throw new Error(message);
    }
    this.setConnecting(true, 'OAuth token broker 호출 중...');
    try {
      const response = await fetch(this.settings.oauthEndpoint, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          provider: this.settings.provider,
          client_id: this.settings.oauthClientId,
          scope: this.settings.oauthScope,
        }),
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const data = await response.json() as { access_token?: string; token?: string };
      const token = data.access_token ?? data.token ?? '';
      if (!token) throw new Error('access_token 없음');
      this.bearerTokenEl.value = token;
      this.authModeEl.value = 'bearer';
      this.readSettingsFromControls();
      saveSettings(this.settings);
      this.updateVisibleSettings();
      this.setStatus('OAuth 토큰 저장됨');
      this.setConnectionState('connected', 'OAuth 토큰을 저장했습니다. 메시지를 보낼 수 있습니다.');
      return this.publicSettings();
    } catch (error) {
      const message = friendlyAiError(error, this.settings);
      this.setStatus('OAuth 실패');
      this.setConnectionState('failed', message);
      this.addMessage('system', `OAuth 실패: ${message}`);
      throw error;
    } finally {
      this.setConnecting(false);
    }
  }

  private async testConnection(): Promise<AiChatPublicSettings> {
    this.readSettingsFromControls();
    saveSettings(this.settings);
    if (!this.credential()) {
      const message = this.settings.authMode === 'apiKey'
        ? `${PROVIDER_LABELS[this.settings.provider]} API key를 입력하세요.`
        : 'OAuth 토큰이 없습니다. 먼저 OAuth 로그인을 실행하세요.';
      this.setConnectionState('failed', message);
      this.setStatus('연결 실패');
      this.addMessage('system', message);
      throw new Error(message);
    }
    this.setConnecting(true, '연결 테스트 중...');
    try {
      await this.callProvider('연결 테스트입니다. OK 한 단어만 답하세요.', 'Connection test. No document context is required.');
      this.setStatus('연결됨');
      this.setConnectionState('connected', `${PROVIDER_LABELS[this.settings.provider]} 연결 테스트 성공`);
      this.addMessage('system', `${PROVIDER_LABELS[this.settings.provider]} 연결 테스트 성공`);
      return this.publicSettings();
    } catch (error) {
      const message = friendlyAiError(error, this.settings);
      this.setStatus('연결 실패');
      this.setConnectionState('failed', message);
      this.addMessage('system', `연결 테스트 실패: ${message}`);
      throw error;
    } finally {
      this.setConnecting(false);
    }
  }

  private buildDocumentContext(): string {
    const { wasm } = this.options;
    const lines: string[] = [
      'Document context:',
      `- file: ${wasm.fileName}`,
      `- source_format: ${wasm.getSourceFormat()}`,
      `- page_count: ${wasm.pageCount}`,
    ];

    if (wasm.pageCount > 0) {
      const page = Math.min(this.options.getCurrentPage(), Math.max(0, wasm.pageCount - 1));
      lines.push(`- current_page: ${page + 1}`);
      try {
        lines.push(`- page_info: ${JSON.stringify(wasm.getPageInfo(page))}`);
      } catch {
        lines.push('- page_info: unavailable');
      }
    }

    const selection = this.readSelectionText();
    if (selection) {
      lines.push('', 'Selected text:', trimContext(selection, this.settings.maxContextChars));
      return lines.join('\n');
    }

    const nearby = this.readNearbyParagraphText();
    if (nearby) {
      lines.push('', 'Cursor paragraph:', trimContext(nearby, this.settings.maxContextChars));
    }
    return trimContext(lines.join('\n'), this.settings.maxContextChars);
  }

  private readSelectionText(): string {
    const handler = this.options.getInputHandler();
    const selection = handler?.getSelection();
    if (!selection) return '';
    const { start, end } = selection;
    try {
      if (isPlainBodyRange(start, end)) {
        return this.options.wasm.copySelection(
          start.sectionIndex,
          start.paragraphIndex,
          start.charOffset,
          end.paragraphIndex,
          end.charOffset,
        );
      }
      if (isSameCellRange(start, end)) {
        return this.options.wasm.copySelectionInCell(
          start.sectionIndex,
          start.parentParaIndex!,
          start.controlIndex!,
          start.cellIndex!,
          start.cellParaIndex!,
          start.charOffset,
          end.cellParaIndex!,
          end.charOffset,
        );
      }
    } catch (error) {
      console.warn('[ai-chat] selection context failed:', error);
    }
    return '';
  }

  private readNearbyParagraphText(): string {
    const position = this.options.getInputHandler()?.getCursorPosition();
    if (!position) return '';
    try {
      if (position.parentParaIndex !== undefined && position.controlIndex !== undefined && position.cellIndex !== undefined) {
        const cellPara = position.cellParaIndex ?? 0;
        const len = this.options.wasm.getCellParagraphLength(
          position.sectionIndex,
          position.parentParaIndex,
          position.controlIndex,
          position.cellIndex,
          cellPara,
        );
        return this.options.wasm.getTextInCell(
          position.sectionIndex,
          position.parentParaIndex,
          position.controlIndex,
          position.cellIndex,
          cellPara,
          0,
          Math.min(len, this.settings.maxContextChars),
        );
      }
      const len = this.options.wasm.getParagraphLength(position.sectionIndex, position.paragraphIndex);
      return this.options.wasm.getTextRange(
        position.sectionIndex,
        position.paragraphIndex,
        0,
        Math.min(len, this.settings.maxContextChars),
      );
    } catch (error) {
      console.warn('[ai-chat] nearby context failed:', error);
      return '';
    }
  }

  private async callProvider(prompt: string, documentContext: string): Promise<string> {
    switch (this.settings.provider) {
      case 'openai':
        return this.callOpenAI(prompt, documentContext);
      case 'anthropic':
        return this.callAnthropic(prompt, documentContext);
      case 'gemini':
        return this.callGemini(prompt, documentContext);
      case 'custom':
        return this.callCustom(prompt, documentContext);
    }
  }

  private async callOpenAI(prompt: string, documentContext: string): Promise<string> {
    const credential = this.credential();
    if (!credential) throw new Error('OpenAI credential is empty');
    const input = this.buildProviderInput(prompt, documentContext);
    const response = await fetch('https://api.openai.com/v1/responses', {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        authorization: `Bearer ${credential}`,
      },
      body: JSON.stringify({
        model: this.settings.model,
        instructions: systemPrompt(),
        input,
      }),
    });
    const data = await readJsonResponse(response);
    return extractOpenAIText(data);
  }

  private async callAnthropic(prompt: string, documentContext: string): Promise<string> {
    const credential = this.credential();
    if (!credential) throw new Error('Claude credential is empty');
    const headers: Record<string, string> = {
      'content-type': 'application/json',
      'anthropic-version': '2023-06-01',
    };
    if (this.settings.authMode === 'apiKey') {
      headers['x-api-key'] = credential;
    } else {
      headers.authorization = `Bearer ${credential}`;
    }
    const response = await fetch('https://api.anthropic.com/v1/messages', {
      method: 'POST',
      headers,
      body: JSON.stringify({
        model: this.settings.model,
        max_tokens: 1600,
        system: systemPrompt(),
        messages: [{ role: 'user', content: this.buildProviderInput(prompt, documentContext) }],
      }),
    });
    const data = await readJsonResponse(response);
    return extractAnthropicText(data);
  }

  private async callGemini(prompt: string, documentContext: string): Promise<string> {
    const credential = this.credential();
    if (!credential) throw new Error('Gemini credential is empty');
    const input = `${systemPrompt()}\n\n${this.buildProviderInput(prompt, documentContext)}`;
    if (window.rhwpDesktop?.geminiGenerate) {
      const data = await window.rhwpDesktop.geminiGenerate({
        model: this.settings.model,
        input,
        apiKey: this.settings.authMode === 'apiKey' ? credential : undefined,
        bearerToken: this.settings.authMode === 'bearer' ? credential : undefined,
      });
      return extractGeminiText(data);
    }
    const response = await fetch('/api/ai/gemini', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        model: this.settings.model,
        input,
        apiKey: this.settings.authMode === 'apiKey' ? credential : undefined,
        bearerToken: this.settings.authMode === 'bearer' ? credential : undefined,
      }),
    });
    const data = await readJsonResponse(response);
    return extractGeminiText(data);
  }


  private async callCustom(prompt: string, documentContext: string): Promise<string> {
    if (!this.settings.customEndpoint) throw new Error('Custom endpoint is empty');
    const headers: Record<string, string> = { 'content-type': 'application/json' };
    const credential = this.credential();
    if (credential) headers.authorization = `Bearer ${credential}`;
    const response = await fetch(this.settings.customEndpoint, {
      method: 'POST',
      headers,
      body: JSON.stringify({
        provider: this.settings.provider,
        model: this.settings.model,
        messages: [
          { role: 'system', content: systemPrompt() },
          { role: 'user', content: this.buildProviderInput(prompt, documentContext) },
        ],
        prompt,
        documentContext,
      }),
    });
    const data = await readJsonResponse(response);
    return extractGenericText(data);
  }

  private credential(): string {
    return this.settings.authMode === 'apiKey'
      ? this.settings.apiKey
      : this.settings.bearerToken;
  }

  private applyEditorActions(actions: AiEditorAction[]): string[] {
    if (actions.length === 0) return [];
    const handler = this.options.getInputHandler();
    if (!handler || this.options.wasm.pageCount <= 0) {
      return ['문서가 열려 있지 않아 편집 동작은 적용하지 못했습니다.'];
    }
    const results: string[] = [];
    for (const action of actions) {
      try {
        switch (action.type) {
          case 'insert_text':
            this.insertTextAtCursor(action.text);
            results.push('커서 위치에 텍스트를 삽입했습니다.');
            break;
          case 'replace_selection':
            results.push(this.replaceSelectionOrInsert(action.text)
              ? '선택 영역을 요청한 텍스트로 교체했습니다.'
              : '선택 영역이 없어 커서 위치에 텍스트를 삽입했습니다.');
            break;
          case 'create_table':
            this.createTableAtCursor(action);
            results.push(`${action.rows}x${action.cols} 표를 생성했습니다.`);
            break;
        }
      } catch (error) {
        results.push(`편집 적용 실패: ${errorMessage(error)}`);
      }
    }
    return results;
  }

  private insertTextAtCursor(text: string): void {
    const handler = this.requireInputHandler();
    const lines = text.replace(/\r\n/g, '\n').split('\n');
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index];
      if (line) {
        handler.executeOperation({
          kind: 'command',
          command: new InsertTextCommand(handler.getCursorPosition(), line),
          meta: { domain: 'text', refresh: 'auto', dirtyScope: 'paragraph' },
        });
      }
      if (index < lines.length - 1) {
        const pos = handler.getCursorPosition();
        handler.executeOperation({
          kind: 'command',
          command: pos.parentParaIndex !== undefined
            ? new SplitParagraphInCellCommand(pos)
            : new SplitParagraphCommand(pos),
          meta: { domain: 'text', refresh: 'auto', dirtyScope: 'paragraph' },
        });
      }
    }
  }

  private replaceSelectionOrInsert(text: string): boolean {
    const handler = this.requireInputHandler();
    const selection = handler.getSelection();
    if (selection) {
      handler.executeOperation({
        kind: 'command',
        command: new DeleteSelectionCommand(selection.start, selection.end),
        meta: { domain: 'text', refresh: 'auto', dirtyScope: 'paragraph' },
      });
    }
    this.insertTextAtCursor(text);
    return Boolean(selection);
  }

  private createTableAtCursor(action: Extract<AiEditorAction, { type: 'create_table' }>): void {
    const handler = this.requireInputHandler();
    const pos = handler.getCursorPosition();
    if (pos.parentParaIndex !== undefined) {
      throw new Error('표 안에서는 새 표 생성을 지원하지 않습니다. 본문 위치로 커서를 옮겨주세요.');
    }
    const rows = clampInt(action.rows, 1, 20, 2);
    const cols = clampInt(action.cols, 1, 12, 2);
    const cells = normalizeTableCells(action.cells, rows, cols);
    handler.executeOperation({
      kind: 'snapshot',
      operationType: 'aiCreateTable',
      operation: (wasm) => {
        const result = wasm.createTableEx({
          sectionIdx: pos.sectionIndex,
          paraIdx: pos.paragraphIndex,
          charOffset: pos.charOffset,
          rowCount: rows,
          colCount: cols,
          treatAsChar: action.treatAsChar ?? true,
        });
        if (!result.ok) return pos;
        if (cells.length > 0) {
          const bboxes = wasm.getTableCellBboxes(pos.sectionIndex, result.paraIdx, result.controlIdx);
          for (let row = 0; row < rows; row += 1) {
            for (let col = 0; col < cols; col += 1) {
              const text = cells[row]?.[col]?.trim();
              if (!text) continue;
              const cell = bboxes.find((box) => box.row === row && box.col === col);
              if (!cell) continue;
              wasm.insertTextInCell(pos.sectionIndex, result.paraIdx, result.controlIdx, cell.cellIdx, 0, 0, text);
            }
          }
        }
        return {
          sectionIndex: pos.sectionIndex,
          paragraphIndex: 0,
          charOffset: 0,
          parentParaIndex: result.paraIdx,
          controlIndex: result.controlIdx,
          cellIndex: 0,
          cellParaIndex: 0,
        };
      },
      meta: { domain: 'table', refresh: 'full', dirtyScope: 'table' },
    });
  }

  private requireInputHandler(): InputHandler {
    const handler = this.options.getInputHandler();
    if (!handler) throw new Error('편집기가 준비되지 않았습니다.');
    return handler;
  }

  private buildProviderInput(prompt: string, documentContext: string): string {
    const chatTurns = this.messages
      .filter((message) => message.role === 'user' || message.role === 'assistant');
    const lastTurn = chatTurns.length > 0 ? chatTurns[chatTurns.length - 1] : undefined;
    const historyTurns = lastTurn?.role === 'user' && lastTurn.text === prompt
      ? chatTurns.slice(0, -1)
      : chatTurns;
    const priorTurns = historyTurns.slice(-8);
    const lines: string[] = [
      documentContext,
    ];
    if (priorTurns.length > 0) {
      lines.push('', 'Conversation history:');
      for (const message of priorTurns) {
        lines.push(`${message.role === 'user' ? 'User' : 'Assistant'}: ${trimContext(message.text, 1800)}`);
      }
    }
    lines.push('', 'Current user request:', prompt);
    return trimContext(lines.join('\n'), this.settings.maxContextChars + 6000);
  }

  private addMessage(role: ChatMessage['role'], text: string): void {
    const message: ChatMessage = { role, text };
    this.messages.push(message);
    this.emptyStateEl.hidden = this.messages.length > 0;
    const item = document.createElement('div');
    item.className = `ai-chat-message ${role}`;
    const label = document.createElement('span');
    label.className = 'ai-chat-message-role';
    label.textContent = role === 'user' ? '나' : role === 'assistant' ? 'AI' : '상태';
    const body = document.createElement('div');
    body.className = 'ai-chat-message-body';
    body.textContent = text;
    item.append(label, body);
    this.messagesEl.appendChild(item);
    this.messagesEl.scrollTop = this.messagesEl.scrollHeight;
  }

  private setSending(value: boolean): void {
    this.sending = value;
    this.sendBtn.disabled = value;
    this.statusEl.textContent = value ? '요청 중...' : '';
    this.refreshDocumentState();
  }

  private setConnecting(value: boolean, detail?: string): void {
    this.connecting = value;
    this.saveSettingsBtn.disabled = value;
    this.testConnectionBtn.disabled = value;
    this.refreshTokenBtn.disabled = value;
    if (value) {
      this.setConnectionState('testing', detail ?? '연결 확인 중...');
    }
  }

  private setStatus(text: string): void {
    this.statusEl.textContent = text;
  }

  private resizePromptInput(): void {
    this.promptEl.style.height = 'auto';
    this.promptEl.style.height = `${Math.min(this.promptEl.scrollHeight, 132)}px`;
  }

  private initializePanelWidth(): void {
    const stored = Number(localStorage.getItem(WIDTH_STORAGE_KEY));
    if (Number.isFinite(stored)) {
      this.setPanelWidth(stored);
    }
  }

  private bindResizeHandle(): void {
    this.resizeHandleEl.addEventListener('pointerdown', (event) => {
      event.preventDefault();
      this.resizeHandleEl.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const startWidth = this.root.getBoundingClientRect().width;
      const onMove = (moveEvent: PointerEvent) => {
        const nextWidth = startWidth + startX - moveEvent.clientX;
        this.setPanelWidth(nextWidth);
      };
      const onUp = () => {
        this.resizeHandleEl.removeEventListener('pointermove', onMove);
        this.resizeHandleEl.removeEventListener('pointerup', onUp);
        localStorage.setItem(WIDTH_STORAGE_KEY, String(Math.round(this.root.getBoundingClientRect().width)));
      };
      this.resizeHandleEl.addEventListener('pointermove', onMove);
      this.resizeHandleEl.addEventListener('pointerup', onUp, { once: true });
    });
  }

  private setPanelWidth(width: number): void {
    const clamped = clampInt(width, MIN_PANEL_WIDTH, MAX_PANEL_WIDTH, 420);
    const editorArea = this.root.closest<HTMLElement>('#editor-area');
    editorArea?.style.setProperty('--ai-panel-width', `${clamped}px`);
  }

  private query<T extends HTMLElement>(role: string, ctor: { new(): T }): T {
    const element = this.root.querySelector(`[data-role="${role}"]`);
    if (!(element instanceof ctor)) {
      throw new Error(`AI chat element not found: ${role}`);
    }
    return element;
  }
}

function loadSettings(): AiChatSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw) as Partial<AiChatSettings>;
    const settings: AiChatSettings = {
      ...DEFAULT_SETTINGS,
      ...parsed,
      provider: isProvider(parsed.provider) ? parsed.provider : DEFAULT_SETTINGS.provider,
      authMode: parsed.authMode === 'bearer' ? 'bearer' : 'apiKey',
      maxContextChars: clampInt(Number(parsed.maxContextChars), 1000, 24000, DEFAULT_SETTINGS.maxContextChars),
    };
    if (settings.provider === 'gemini' && settings.model === 'gemini-2.0-flash') {
      settings.model = PROVIDER_DEFAULT_MODELS.gemini;
    }
    return settings;
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function saveSettings(settings: AiChatSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

function isProvider(value: unknown): value is AiProvider {
  return value === 'openai' || value === 'anthropic' || value === 'gemini' || value === 'custom';
}

function isAuthMode(value: unknown): value is AiAuthMode {
  return value === 'apiKey' || value === 'bearer';
}

function stringSetting(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value.trim() : fallback;
}

function clampInt(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function trimContext(value: string, maxChars: number): string {
  if (value.length <= maxChars) return value;
  return `${value.slice(0, maxChars)}\n[context truncated]`;
}

function isPlainBodyRange(start: DocumentPosition, end: DocumentPosition): boolean {
  return start.sectionIndex === end.sectionIndex
    && start.parentParaIndex === undefined
    && end.parentParaIndex === undefined;
}

function isSameCellRange(start: DocumentPosition, end: DocumentPosition): boolean {
  return start.sectionIndex === end.sectionIndex
    && start.parentParaIndex !== undefined
    && start.parentParaIndex === end.parentParaIndex
    && start.controlIndex !== undefined
    && start.controlIndex === end.controlIndex
    && start.cellIndex !== undefined
    && start.cellIndex === end.cellIndex
    && start.cellParaIndex !== undefined
    && end.cellParaIndex !== undefined;
}

function systemPrompt(): string {
  return [
    'You are an AI assistant embedded in rhwp-studio, a HWP/HWPX editor.',
    'Use the provided document context as evidence.',
    'Answer in Korean unless the user asks for another language.',
    'The host editor can apply a limited set of actions when you return an rhwpActions JSON block.',
    'Available action objects: {"type":"insert_text","text":"..."}, {"type":"replace_selection","text":"..."}, {"type":"create_table","rows":2,"cols":3,"cells":[["..."]]}',
    'When the user asks to edit the document and the request is specific enough, include one fenced JSON block at the end: {"message":"brief Korean summary","rhwpActions":[...]}',
    'Do not claim unsupported formatting or layout changes were applied. Ask a follow-up question if the target or content is ambiguous.',
  ].join('\n');
}

async function readJsonResponse(response: Response): Promise<unknown> {
  const text = await response.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = { text };
    }
  }
  if (!response.ok) {
    const detail = extractErrorText(data) || response.statusText;
    throw new Error(`HTTP ${response.status}: ${detail}`);
  }
  return data;
}

function extractErrorText(data: unknown): string {
  if (!isRecord(data)) return typeof data === 'string' ? data : '';
  const error = data.error;
  if (typeof error === 'string') return error;
  if (isRecord(error)) {
    const message = typeof error.message === 'string' ? error.message : '';
    const status = typeof error.status === 'string' ? error.status : '';
    return [status, message].filter(Boolean).join(': ');
  }
  return extractGenericText(data);
}

function extractOpenAIText(data: unknown): string {
  if (isRecord(data) && typeof data.output_text === 'string') return data.output_text;
  if (!isRecord(data) || !Array.isArray(data.output)) return extractGenericText(data);
  const parts: string[] = [];
  for (const output of data.output) {
    if (!isRecord(output) || !Array.isArray(output.content)) continue;
    for (const content of output.content) {
      if (isRecord(content) && typeof content.text === 'string') parts.push(content.text);
    }
  }
  return parts.join('\n').trim() || extractGenericText(data);
}

function extractAnthropicText(data: unknown): string {
  if (!isRecord(data) || !Array.isArray(data.content)) return extractGenericText(data);
  const parts = data.content
    .filter(isRecord)
    .map((item) => typeof item.text === 'string' ? item.text : '')
    .filter(Boolean);
  return parts.join('\n').trim() || extractGenericText(data);
}

function extractGeminiText(data: unknown): string {
  if (isRecord(data) && typeof data.output_text === 'string') return data.output_text;
  if (isRecord(data) && typeof data.outputText === 'string') return data.outputText;
  if (isRecord(data) && Array.isArray(data.output)) {
    const parts = collectTextFields(data.output);
    if (parts.length > 0) return parts.join('\n').trim();
  }
  if (isRecord(data) && Array.isArray(data.steps)) {
    const parts = collectGeminiStepText(data.steps);
    if (parts.length > 0) return parts.join('\n').trim();
  }
  if (isRecord(data) && Array.isArray(data.candidates)) {
    const parts: string[] = [];
    for (const candidate of data.candidates) {
      if (!isRecord(candidate) || !isRecord(candidate.content) || !Array.isArray(candidate.content.parts)) continue;
      for (const part of candidate.content.parts) {
        if (isRecord(part) && typeof part.text === 'string') parts.push(part.text);
      }
    }
    const text = parts.join('\n').trim();
    if (text) return text;
  }
  return extractGenericText(data);
}

function extractGenericText(data: unknown): string {
  if (typeof data === 'string') return data;
  if (!isRecord(data)) return '';
  for (const key of ['text', 'output_text', 'outputText', 'message', 'answer', 'response']) {
    const value = data[key];
    if (typeof value === 'string') return value;
  }
  const content = data.content;
  if (Array.isArray(content)) {
    const parts = collectTextFields(content);
    if (parts.length > 0) return parts.join('\n').trim();
  } else if (isRecord(content)) {
    const parts = collectTextFields([content]);
    if (parts.length > 0) return parts.join('\n').trim();
  }
  const output = data.output;
  if (Array.isArray(output)) {
    const parts = collectTextFields(output);
    if (parts.length > 0) return parts.join('\n').trim();
  }
  return '응답 텍스트를 찾지 못했습니다. provider 응답 형식을 확인하세요.';
}

function collectGeminiStepText(steps: unknown[]): string[] {
  const parts: string[] = [];
  for (const step of steps) {
    if (!isRecord(step)) continue;
    for (const key of ['modelOutput', 'model_output', 'output']) {
      const output = step[key];
      if (!isRecord(output)) continue;
      const content = output.content;
      if (Array.isArray(content)) parts.push(...collectTextFields(content));
      else if (isRecord(content)) parts.push(...collectTextFields([content]));
      parts.push(...collectTextFields([output]));
    }
  }
  return parts.filter(Boolean);
}

function collectTextFields(values: unknown[]): string[] {
  const parts: string[] = [];
  for (const value of values) {
    if (typeof value === 'string') {
      parts.push(value);
      continue;
    }
    if (!isRecord(value)) continue;
    const text = value.text;
    if (typeof text === 'string') {
      parts.push(text);
    } else if (isRecord(text) && typeof text.text === 'string') {
      parts.push(text.text);
    }
    const outputText = value.outputText ?? value.output_text;
    if (typeof outputText === 'string') parts.push(outputText);
    const message = value.message;
    if (typeof message === 'string') parts.push(message);
    else if (isRecord(message)) parts.push(...collectTextFields([message]));
    const partsValue = value.parts;
    if (Array.isArray(partsValue)) parts.push(...collectTextFields(partsValue));
    const content = value.content;
    if (Array.isArray(content)) parts.push(...collectTextFields(content));
    else if (isRecord(content)) parts.push(...collectTextFields([content]));
    const output = value.output;
    if (Array.isArray(output)) parts.push(...collectTextFields(output));
    else if (isRecord(output)) parts.push(...collectTextFields([output]));
  }
  return Array.from(new Set(parts.map((part) => part.trim()).filter(Boolean)));
}

function extractEditorActions(text: string): { text: string; actions: AiEditorAction[] } {
  const payloads: AiEditorActionPayload[] = [];
  const fencedPattern = /```(?:json)?\s*([\s\S]*?)```/gi;
  let cleaned = text;
  let match: RegExpExecArray | null;
  while ((match = fencedPattern.exec(text))) {
    const parsed = parseActionPayload(match[1]);
    if (!parsed) continue;
    payloads.push(parsed);
    cleaned = cleaned.replace(match[0], '');
  }

  const inlinePayload = parseActionPayload(cleaned);
  if (inlinePayload) {
    payloads.push(inlinePayload);
    cleaned = '';
  }

  const actions = payloads.flatMap((payload) => normalizeEditorActions(payload.rhwpActions));
  const message = payloads.map((payload) => payload.message).find((value): value is string => Boolean(value?.trim()));
  return {
    text: cleaned.trim() || message || '',
    actions,
  };
}

function parseActionPayload(value: string): AiEditorActionPayload | null {
  try {
    const parsed = JSON.parse(value.trim()) as unknown;
    if (!isRecord(parsed) || !Array.isArray(parsed.rhwpActions)) return null;
    return {
      message: typeof parsed.message === 'string' ? parsed.message : undefined,
      rhwpActions: parsed.rhwpActions,
    };
  } catch {
    return null;
  }
}

function normalizeEditorActions(value: unknown): AiEditorAction[] {
  if (!Array.isArray(value)) return [];
  const actions: AiEditorAction[] = [];
  for (const item of value) {
    if (!isRecord(item) || typeof item.type !== 'string') continue;
    if ((item.type === 'insert_text' || item.type === 'replace_selection') && typeof item.text === 'string') {
      actions.push({ type: item.type, text: item.text });
      continue;
    }
    if (item.type === 'create_table') {
      const rows = Number(item.rows);
      const cols = Number(item.cols);
      if (!Number.isFinite(rows) || !Number.isFinite(cols)) continue;
      actions.push({
        type: 'create_table',
        rows,
        cols,
        cells: normalizeUnknownCells(item.cells),
        treatAsChar: typeof item.treatAsChar === 'boolean' ? item.treatAsChar : undefined,
      });
    }
  }
  return actions;
}

function normalizeUnknownCells(value: unknown): string[][] | undefined {
  if (!Array.isArray(value)) return undefined;
  return value.map((row) => Array.isArray(row)
    ? row.map((cell) => typeof cell === 'string' ? cell : String(cell ?? ''))
    : []);
}

function normalizeTableCells(value: string[][] | undefined, rows: number, cols: number): string[][] {
  if (!value) return [];
  return Array.from({ length: rows }, (_, row) =>
    Array.from({ length: cols }, (_, col) => value[row]?.[col] ?? ''),
  );
}

function composeAssistantMessage(text: string, applied: string[]): string {
  const parts: string[] = [];
  if (text.trim()) parts.push(text.trim());
  if (applied.length > 0) {
    parts.push(`적용 결과: ${applied.join(' ')}`);
  }
  return parts.join('\n\n') || '완료했습니다.';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function friendlyAiError(error: unknown, settings: AiChatSettings): string {
  const raw = errorMessage(error);
  if (raw === 'Failed to fetch' || raw.includes('Load failed')) {
    return `${PROVIDER_LABELS[settings.provider]} 요청이 브라우저에서 차단되었거나 네트워크가 실패했습니다. 운영 환경에서는 서버 프록시 또는 token broker를 권장합니다.`;
  }
  if (settings.provider === 'gemini') {
    if (/HTTP 404/i.test(raw)) {
      return 'Gemini proxy endpoint가 없습니다. 로컬 vite dev/preview에서는 /api/ai/gemini가 제공되며, 정적 배포에서는 같은 역할의 서버 proxy를 연결해야 합니다.';
    }
    if (/API key not valid|API_KEY_INVALID|INVALID_ARGUMENT|permission|credential|unauthenticated/i.test(raw)) {
      return 'Gemini API key가 유효하지 않거나 Generative Language API 권한/제한 설정이 맞지 않습니다. Google AI Studio에서 생성한 Gemini API key인지 확인하고, 키 제한이 있다면 generativelanguage.googleapis.com 호출을 허용하세요.';
    }
    if (/quota|rate/i.test(raw)) {
      return 'Gemini 사용량 한도 또는 rate limit에 걸렸습니다. Google AI Studio의 quota/billing 상태를 확인하세요.';
    }
  }
  if (/credential is empty/i.test(raw)) {
    return settings.authMode === 'apiKey'
      ? `${PROVIDER_LABELS[settings.provider]} API key를 입력하세요.`
      : 'OAuth bearer token이 없습니다. OAuth 로그인을 먼저 실행하세요.';
  }
  return raw;
}
