import { http } from "./client";

export type AiMultimodalInputType = "image" | "video" | "audio" | "file";
export type AiMultimodalInputTransport = "url" | "base64";
export type ToolExecutionMode =
  | "DEFAULT"
  | "ALWAYS_ASK"
  | "ALWAYS_ALLOW"
  | "FULL_ACCESS";

export interface AiMultimodalInput {
  id: string;
  name: string;
  inputType: AiMultimodalInputType;
  mimeType: string;
  transport: AiMultimodalInputTransport;
  url?: string;
  data?: string;
  /** 应用存储中的持久化地址，用于用户消息回显，不发送给模型。 */
  resourceUrl?: string;
  size: number;
}

// ========== 类型定义 ==========

export interface AiChatReq {
  message?: string;
  conversationId?: string;
  modelId?: number;
  reasoningEffort?: string;
  agentType?: string;
  category?: string;
  /** 自定义对话标题（不传则使用消息前50字） */
  title?: string;
  projectId?: number;
  context?: Record<string, unknown>;
  systemPrompt?: string;
  instruction?: string;
  enabledTools?: string[];
  /** 用户在输入区显式激活的 Skill 名称。 */
  enabledSkills?: string[];
  enabledMcpTools?: string[];
  multimodalInputs?: AiMultimodalInput[];
  enableParallelTools?: boolean;
  toolExecutionMode: ToolExecutionMode;
  referencesJson?: string;
  /** 当前页面上下文引用（type + id），用于模板变量替换 */
  autoReferences?: Array<{ type: string; id: number }>;
}

export type OutputType =
  | "REASONING"
  | "CONTENT"
  | "TOOL_CALL_STARTED"
  | "TOOL_CALL"
  | "TOOL_FINISHED"
  | "SUB_AGENT_STARTED"
  | "SUB_AGENT_FINISHED"
  | "USER_CONFIRMATION_REQUIRED"
  | "EXTERNAL_EXECUTION_REQUIRED"
  | "USER_CONFIRM_RESULT"
  | "EXTERNAL_EXECUTION_RESULT"
  | "DONE"
  | "ERROR"
  | "CANCELLED";

export interface ToolCallInfo {
  id: string;
  name: string;
  arguments: string;
}

export interface ToolConfirmationDecision {
  toolCallId: string;
  approved: boolean;
}

export interface AiChatStreamEvent {
  /** Durable fields are present on AgentScope v2 Pipeline events. */
  schemaVersion?: number;
  runId?: string;
  sequence?: number;
  messageId?: string;
  conversationId?: string;
  outputType: OutputType;
  content?: string;
  reasoningContent?: string;
  reasoningStartTime?: number | null;
  reasoningDurationMs?: number | null;
  toolCalls?: ToolCallInfo[];
  toolCallId?: string;
  toolName?: string;
  toolResult?: string | null;
  toolStatus?: string;
  finished?: boolean;
  error?: string;
  /** 子 Agent 事件关联的父级工具调用 ID */
  parentToolCallId?: string;
  /** 事件来源 Agent 名称（null 表示主 Agent） */
  agentName?: string;
  source?: string;
  replyId?: string;
  blockId?: string;
  rawEventId?: string;
  rawEventType?: string;
  createdAt?: string;
  controlType?: string;
  pendingToolCalls?: Array<{
    toolCallId: string;
    toolName: string;
    argumentsPreview: string;
  }>;
  decisions?: ToolConfirmationDecision[];
  expiresAt?: string;
  cancellationReason?: string;
}

// ========== SSE 回调 ==========

export interface StreamCallbacks {
  onEvent: (event: AiChatStreamEvent) => void;
  onError?: (error: Error) => void;
  onComplete?: () => void;
}

// ========== SSE 流式专用工具函数 ==========

/**
 * 从 localStorage 读取 token（仅供 SSE 流式 fetch 使用）
 */
/**
 * SSE 请求包装。
 *
 * 桌面版单机单用户，没有登录态与令牌刷新：直接同源请求即可，
 * 保留该函数是为了让调用方（流水线、任务流）无需改动。
 */
export async function authenticatedFetch(
  input: RequestInfo | URL,
  init?: RequestInit
): Promise<Response> {
  return fetch(input, init);
}

// ========== 对话历史 API ==========

export interface AgentConversation {
  id: number;
  conversationId: string;
  userId: number;
  projectId: number | null;
  contextType?: string;
  agentType?: string;
  category?: string;
  contextId?: number;
  title: string;
  messageCount: number;
  lastMessageTime?: string;
  status: string;
  agentStateStatus?: "ACTIVE" | "EXPIRED";
  createTime?: string;
}

export interface AgentMessage {
  id: number;
  conversationId: string;
  runId?: string;
  projectionKey?: string;
  role: string;
  content: string;
  referencesJson?: string;
  toolName?: string;
  toolStatus?: string;
  /** 工具调用 ID（关联同一次调用的发起和结果） */
  toolCallId?: string;
  /** 父级工具调用 ID（子 Agent 事件归属） */
  parentToolCallId?: string;
  reasoningContent?: string;
  reasoningDurationMs?: number | null;
  messageOrder: number;
  createTime?: string;
}

export interface PageResult<T> {
  list: T[];
  total: number;
}

export interface AssistantSkillReferenceOption {
  id: string;
  name: string;
  displayName: string;
  description: string;
  source: string;
}

export interface AssistantMcpToolReferenceOption {
  serverName: string;
  toolName: string;
  description: string;
  readOnly: boolean;
}

export interface AssistantReferenceOptions {
  skills: AssistantSkillReferenceOption[];
  mcpTools: AssistantMcpToolReferenceOption[];
}

export async function getAssistantReferenceOptions(): Promise<AssistantReferenceOptions> {
  const options = await http.get<unknown, Partial<AssistantReferenceOptions>>(
    "/api/ai/assistant/reference-options"
  );
  return {
    skills: Array.isArray(options?.skills) ? options.skills : [],
    mcpTools: Array.isArray(options?.mcpTools) ? options.mcpTools : [],
  };
}

/**
 * 获取对话列表（分页）
 */
export async function listConversations(params: {
  pageNo: number;
  pageSize: number;
  category?: string;
}): Promise<PageResult<AgentConversation>> {
  const searchParams = new URLSearchParams({
    pageNo: String(params.pageNo),
    pageSize: String(params.pageSize),
  });
  if (params.category) {
    searchParams.set("category", params.category);
  }
  return http.get(`/api/ai/assistant/conversations?${searchParams}`);
}

/**
 * 获取对话消息列表
 */
export async function listMessages(
  conversationId: string
): Promise<AgentMessage[]> {
  return http.get(
    `/api/ai/assistant/conversations/${encodeURIComponent(conversationId)}/messages`
  );
}

/**
 * 删除对话
 */
export async function deleteConversation(id: number): Promise<void> {
  await http.delete(`/api/ai/assistant/conversations/${id}`);
}

/**
 * 按稳定会话标识幂等删除，兼容尚未同步数据库 ID 的乐观会话。
 */
export async function deleteConversationByConversationId(
  conversationId: string
): Promise<void> {
  await http.delete(
    `/api/ai/assistant/conversations/by-conversation-id/${encodeURIComponent(conversationId)}`
  );
}

