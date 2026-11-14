import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react'
import type { ChatMessage, ToolCall, ToolCallStatus } from '../chat'

interface ChatPanelProps {
  messages: ChatMessage[]
  onSend: (text: string) => void
}

export function ChatPanel({ messages, onSend }: ChatPanelProps) {
  const [draft, setDraft] = useState('')
  const listRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const list = listRef.current
    if (list) list.scrollTop = list.scrollHeight
  }, [messages])

  const submit = () => {
    const text = draft.trim()
    if (!text) return
    onSend(text)
    setDraft('')
  }

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault()
    submit()
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault()
      submit()
    }
  }

  return (
    <section
      aria-label="Assistant"
      className="flex min-h-0 flex-1 flex-col rounded-lg border border-hairline bg-white"
    >
      <header className="flex h-12 flex-none items-center border-b border-hairline px-4 font-semibold">
        Assistant
      </header>

      <div ref={listRef} className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
        {messages.length === 0 ? (
          <p className="leading-normal text-muted">
            Describe the space you want, for example “Furnish this as a restaurant for 20 people.”
          </p>
        ) : (
          messages.map((message) =>
            message.role === 'user' ? (
              <UserMessage key={message.id} text={message.text} />
            ) : (
              <AssistantMessage key={message.id} text={message.text} toolCalls={message.toolCalls} />
            ),
          )
        )}
      </div>

      <form onSubmit={handleSubmit} className="flex flex-none flex-col gap-2 border-t border-hairline p-3">
        <textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Describe a change…"
          aria-label="Message the assistant"
          rows={2}
          className="min-h-14 resize-none rounded-md border border-control-border px-2.5 py-2 leading-[1.45] placeholder:text-placeholder focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent/15"
        />
        <div className="flex justify-end">
          <button
            type="submit"
            disabled={!draft.trim()}
            className="h-8 rounded-md bg-ink px-3 font-medium text-white hover:bg-ink/90 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Send
          </button>
        </div>
      </form>
    </section>
  )
}

function UserMessage({ text }: { text: string }) {
  return (
    <div className="max-w-[85%] self-end whitespace-pre-wrap rounded-lg border border-hairline bg-canvas px-3 py-2 leading-[1.45]">
      {text}
    </div>
  )
}

function AssistantMessage({ text, toolCalls }: { text: string; toolCalls?: ToolCall[] }) {
  return (
    <div className="flex flex-col gap-2 leading-normal">
      <span className="whitespace-pre-wrap">{text}</span>
      {toolCalls && toolCalls.length > 0 && (
        <ul className="rounded-md border border-hairline font-mono text-xs leading-[1.4] text-ink-secondary">
          {toolCalls.map((call, index) => (
            <li
              key={index}
              className="flex justify-between border-hairline-soft px-2.5 py-1.5 not-last:border-b"
            >
              <span>{call.name}</span>
              <span className={STATUS_COLOR[call.status]}>{call.status}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

const STATUS_COLOR: Record<ToolCallStatus, string> = {
  running: 'text-accent-strong',
  done: 'text-success',
  passed: 'text-success',
  failed: 'text-danger',
}
