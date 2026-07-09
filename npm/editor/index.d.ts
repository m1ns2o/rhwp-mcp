/**
 * @rhwp/editor — HWP 에디터 웹 컴포넌트
 */

export interface EditorOptions {
  /** rhwp-studio URL (기본: https://edwardkim.github.io/rhwp/) */
  studioUrl?: string;
  /** iframe 너비 (기본: '100%') */
  width?: string;
  /** iframe 높이 (기본: '100%') */
  height?: string;
}

export interface LoadResult {
  pageCount: number;
}

export interface HwpVerifyResult {
  /** 직렬화된 HWP 바이트 수 */
  bytesLen: number;
  /** 직렬화 직전 페이지 수 */
  pageCountBefore: number;
  /** 자기 재로드 후 페이지 수 (recovered === true 일 때 의미 있음) */
  pageCountAfter: number;
  /** 자기 재로드 성공 여부 */
  recovered: boolean;
}

export type AiProvider = 'openai' | 'anthropic' | 'gemini' | 'custom';
export type AiAuthMode = 'apiKey' | 'bearer';

export interface AiSettingsInput {
  /** AI provider */
  provider?: AiProvider;
  /** API key 또는 bearer/OAuth token 사용 방식 */
  authMode?: AiAuthMode;
  /** provider model id */
  model?: string;
  /** API key. configureAi 응답에는 원문이 포함되지 않습니다. */
  apiKey?: string;
  /** bearer/OAuth token. configureAi 응답에는 원문이 포함되지 않습니다. */
  bearerToken?: string;
  /** custom provider endpoint */
  customEndpoint?: string;
  /** token broker endpoint. POST JSON으로 provider/client_id/scope를 받고 access_token 또는 token을 반환해야 합니다. */
  oauthEndpoint?: string;
  /** token broker에 전달할 client_id */
  oauthClientId?: string;
  /** token broker에 전달할 scope */
  oauthScope?: string;
  /** 문서 컨텍스트 최대 글자 수 */
  maxContextChars?: number;
}

export interface AiSettingsSummary {
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

export declare class RhwpEditor {
  /** HWP 파일을 로드합니다 */
  loadFile(data: ArrayBuffer | Uint8Array, fileName?: string): Promise<LoadResult>;
  /** 빈 HWP 문서를 생성합니다 */
  newDocument(): Promise<LoadResult>;
  /** 현재 문서의 페이지 수를 반환합니다 */
  pageCount(): Promise<number>;
  /** 특정 페이지를 SVG 문자열로 렌더링합니다 */
  getPageSvg(page?: number): Promise<string>;
  /** 현재 문서를 HWP 바이너리로 내보냅니다 */
  exportHwp(): Promise<Uint8Array>;
  /** 현재 문서를 HWPX(ZIP+XML) 바이너리로 내보냅니다 */
  exportHwpx(): Promise<Uint8Array>;
  /** HWP 직렬화 + 자기 재로드 검증 메타데이터 (#178) */
  exportHwpVerify(): Promise<HwpVerifyResult>;
  /** AI 패널 provider/API 설정을 등록합니다. 응답에는 secret 원문이 포함되지 않습니다. */
  configureAi(settings: AiSettingsInput): Promise<AiSettingsSummary>;
  /** 현재 AI 패널 설정 요약을 반환합니다. API key/token 원문은 반환하지 않습니다. */
  getAiSettings(): Promise<AiSettingsSummary>;
  /** AI 사이드 패널을 열거나 닫습니다. */
  openAiPanel(open?: boolean): Promise<{ visible: boolean }>;
  /** 설정된 OAuth/token broker URL에서 bearer token을 받아 저장합니다. */
  refreshAiOAuthToken(): Promise<AiSettingsSummary>;
  /** iframe 엘리먼트를 반환합니다 */
  readonly element: HTMLIFrameElement;
  /** 에디터를 제거합니다 */
  destroy(): void;
}

/**
 * HWP 에디터를 생성하여 지정된 컨테이너에 마운트합니다.
 *
 * @example
 * ```javascript
 * import { createEditor } from '@rhwp/editor';
 *
 * const editor = await createEditor('#container');
 * const resp = await fetch('document.hwp');
 * await editor.loadFile(await resp.arrayBuffer());
 * ```
 */
export declare function createEditor(
  container: string | HTMLElement,
  options?: EditorOptions,
): Promise<RhwpEditor>;
