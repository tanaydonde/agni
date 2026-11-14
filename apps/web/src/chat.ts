export type ToolCallStatus = 'running' | 'done' | 'passed' | 'failed'

export interface ToolCall {
  name: string
  status: ToolCallStatus
}

export type ChatMessage =
  | { id: string; role: 'user'; text: string }
  | { id: string; role: 'assistant'; text: string; toolCalls?: ToolCall[] }
