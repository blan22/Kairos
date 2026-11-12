/**
 * Orca 컴포넌트 대역(React). 모양은 Orca 테마를 가깝게 흉내 낼 뿐 같지 않다. 정확한 모양은 실제 렌더 캡처(public/shots)로 본다.
 * 외부 문자열은 Orca처럼 글자로만 그린다. dangerouslySetInnerHTML을 쓰지 않는다.
 */
import { Children as ReactChildren, isValidElement, useState, type ReactElement, type ReactNode } from 'react'

type Gap = 'none' | 'xs' | 'sm' | 'md' | 'lg'
type Tone = 'neutral' | 'accent' | 'success' | 'warning' | 'danger'
type Press = () => unknown
type Children = { children?: ReactNode }

const cx = (...names: (string | false | null | undefined)[]): string => names.filter(Boolean).join(' ')

const ICONS: Record<string, string> = {
  add: '+', remove: '−', delete: '🗑', edit: '✎', refresh: '↻', search: '⌕', settings: '⚙', check: '✓', close: '✕',
  info: 'ℹ', warning: '⚠', error: '⛔', 'open-external': '↗', copy: '⧉', download: '↓', upload: '↑', play: '▶',
  pause: '⏸', stop: '■', more: '⋯', expand: '▾', collapse: '▸', next: '›', filter: '⏷', branch: '⑂', file: '🗎',
  agent: '🤖', link: '🔗', bell: '🔔', bot: '🤖', calendar: '📅', flag: '⚑', zap: 'ϟ', star: '★', terminal: '⌨',
  rocket: '🚀', activity: '∿', clock: '◷', 'file-text': '🗎',
}
export const iconOf = (name?: string): string => (name ? (ICONS[name] ?? '•') : '')

export function Box(props: Children & {
  direction?: 'row' | 'column'; gap?: Gap; padding?: Gap; paddingX?: Gap; paddingY?: Gap; align?: string; justify?: string
  wrap?: boolean; grow?: boolean; shrink?: boolean; border?: boolean | Tone; radius?: string; background?: string; scroll?: string
  maxHeight?: number | string; minWidth?: number | string; width?: number | string
}) {
  const { children, direction = 'column', gap = 'none', padding, paddingX, paddingY, align, justify, wrap, grow, shrink, border, radius, background, scroll } = props
  return (
    <div
      className={cx('o-box', `o-dir-${direction}`, `o-gap-${gap}`, padding && `o-pad-${padding}`, paddingX && `o-padx-${paddingX}`, paddingY && `o-pady-${paddingY}`,
        align && `o-align-${align}`, justify && `o-justify-${justify}`, wrap && 'o-wrap', grow && 'o-grow', shrink === false && 'o-no-shrink', border && 'o-border',
        typeof border === 'string' && `o-tone-border-${border}`, radius && `o-radius-${radius}`, background && `o-bg-${background}`, scroll && `o-scroll-${scroll}`)}
      style={{ maxHeight: props.maxHeight, minWidth: props.minWidth, width: props.width }}
    >
      {children}
    </div>
  )
}

export const Stack = ({ children, gap = 'sm', align }: Children & { gap?: Gap; align?: string }) => <Box gap={gap} align={align}>{children}</Box>
export const Row = ({ children, gap = 'sm', align = 'center', justify, wrap }: Children & { gap?: Gap; align?: string; justify?: string; wrap?: boolean }) => (
  <Box direction="row" gap={gap} align={align} justify={justify} wrap={wrap}>{children}</Box>
)

export const Text = ({ children, tone, muted, size = 'sm', weight, truncate }: Children & { tone?: Tone; muted?: boolean; size?: string; weight?: string; truncate?: boolean }) => (
  <span className={cx('o-text', `o-size-${size}`, tone && `o-tone-${tone}`, muted && 'o-muted', weight && `o-weight-${weight}`, truncate && 'o-truncate')}>{children}</span>
)

export const Heading = ({ children, level = 2 }: Children & { level?: 1 | 2 | 3 }) => <div className={`o-heading o-h${level}`}>{children}</div>

export const Button = ({ children, variant = 'outline', tone, size = 'sm', icon, tooltip, disabled, onPress }: Children & {
  variant?: string; tone?: Tone; size?: string; icon?: string; tooltip?: string; disabled?: boolean; onPress?: Press
}) => (
  <button type="button" className={cx('o-button', `o-variant-${variant}`, tone && `o-tone-${tone}`, `o-size-${size}`)} title={tooltip} disabled={disabled} onClick={() => onPress?.()}>
    {icon ? <span className="o-icon">{iconOf(icon)}</span> : null}
    {children}
  </button>
)

export const IconButton = ({ icon, label, tooltip, variant = 'ghost', tone, disabled, onPress }: {
  icon: string; label: string; tooltip?: string; variant?: string; tone?: Tone; size?: string; disabled?: boolean; onPress?: Press
}) => (
  <button type="button" aria-label={label} title={tooltip ?? label} className={cx('o-button', 'o-icon-button', `o-variant-${variant}`, tone && `o-tone-${tone}`)} disabled={disabled} onClick={() => onPress?.()}>
    {iconOf(icon)}
  </button>
)

export const Badge = ({ children, tone = 'neutral' }: Children & { tone?: Tone }) => <span className={`o-badge o-tone-${tone}`}>{children}</span>

export const Card = ({ children, title, description }: Children & { title?: string; description?: string }) => (
  <section className="o-card">
    {title ? <div className="o-card-title">{title}</div> : null}
    {description ? <div className="o-muted o-size-xs">{description}</div> : null}
    {children}
  </section>
)

export const List = ({ children, label }: Children & { label?: string }) => <div className="o-list" role="list" aria-label={label}>{children}</div>

export const ListItem = ({ children, title, description, icon, current, disabled, onPress }: Children & {
  title?: string; description?: string; icon?: string; current?: boolean; disabled?: boolean; onPress?: Press
}) => (
  <div role="listitem" className={cx('o-list-item', current && 'o-current', disabled && 'o-disabled', onPress && 'o-pressable')} onClick={() => !disabled && onPress?.()}>
    {icon ? <span className="o-icon o-muted">{iconOf(icon)}</span> : null}
    <div className="o-list-body">
      {title ? <div className="o-list-title">{title}</div> : null}
      {description ? <div className="o-muted o-size-xs">{description}</div> : null}
      {children}
    </div>
  </div>
)

export function Input({ label, labelHidden, value, defaultValue, placeholder, maxLength, invalid, disabled, onChange, onSubmit }: {
  label: string; labelHidden?: boolean; value?: string; defaultValue?: string; placeholder?: string; type?: string; maxLength?: number
  invalid?: boolean; disabled?: boolean; onChange?: (value: string) => void; onSubmit?: (value: string) => void
}) {
  const [local, setLocal] = useState(defaultValue ?? '')
  const current = value ?? local
  return (
    <label className="o-field">
      {labelHidden ? null : <span className="o-size-xs o-muted">{label}</span>}
      <input
        aria-label={label}
        className={cx('o-input', invalid && 'o-invalid')}
        value={current}
        placeholder={placeholder}
        maxLength={maxLength}
        disabled={disabled}
        onChange={event => {
          setLocal(event.target.value)
          onChange?.(event.target.value)
        }}
        onKeyDown={event => {
          if (event.key === 'Enter') onSubmit?.(current)
        }}
      />
    </label>
  )
}

export const Separator = () => <hr className="o-separator" />
export const Spinner = ({ label, labelHidden }: { label: string; labelHidden?: boolean }) => (
  <span className="o-spinner" role="status" aria-label={label}>◌{labelHidden ? null : <span className="o-muted o-size-xs"> {label}</span>}</span>
)
export const Progress = ({ label, value }: { label: string; value?: number }) => <progress aria-label={label} value={value} max={1} />

export const EmptyState = ({ children, title, description, icon }: Children & { title: string; description?: string; icon?: string }) => (
  <div className="o-empty">
    {icon ? <div className="o-empty-icon">{iconOf(icon)}</div> : null}
    <div className="o-weight-semibold">{title}</div>
    {description ? <div className="o-muted o-size-xs">{description}</div> : null}
    {children}
  </div>
)

export const Link = ({ children, onPress }: Children & { href?: string; onPress?: Press }) => (
  <a className="o-link" href="#" onClick={event => { event.preventDefault(); onPress?.() }}>{children}</a>
)

export const Code = ({ children, block }: Children & { block?: boolean }) => (block ? <pre className="o-code-block">{children}</pre> : <code className="o-code">{children}</code>)

export const Callout = ({ children, tone = 'neutral', title, icon }: Children & { tone?: Tone; title?: string; icon?: string }) => (
  <div className={`o-callout o-tone-border-${tone}`} role="note">
    {title || icon ? <div className="o-weight-semibold">{iconOf(icon)} {title}</div> : null}
    {children}
  </div>
)

export const Skeleton = ({ shape = 'text', lines = 1, label }: { shape?: string; lines?: number; label?: string }) => (
  <div aria-label={label} className={`o-skeleton o-skeleton-${shape}`}>
    {Array.from({ length: lines }, (_, index) => <div key={index} className="o-skeleton-line" />)}
  </div>
)

function relative(at: number): string {
  const seconds = Math.round((at - Date.now()) / 1000)
  const units: [Intl.RelativeTimeFormatUnit, number][] = [['day', 86_400], ['hour', 3_600], ['minute', 60], ['second', 1]]
  const format = new Intl.RelativeTimeFormat('ko', { numeric: 'auto' })
  for (const [unit, size] of units) if (Math.abs(seconds) >= size || unit === 'second') return format.format(Math.round(seconds / size), unit)
  return ''
}

/** 감싼 Text의 크기를 따른다. 따로 크기를 주면 xs 줄 안에서 시각만 커진다. */
export const RelativeTime = ({ at, tone, muted }: { at: number; format?: string; tone?: Tone; muted?: boolean }) => (
  <span className={cx('o-text', tone && `o-tone-${tone}`, muted && 'o-muted')}>{relative(at)}</span>
)

export const Collapsible = ({ children, title, description, open, defaultOpen, onChange }: Children & {
  title: string; description?: string; open?: boolean; defaultOpen?: boolean; disabled?: boolean; onChange?: (open: boolean) => void
}) => {
  const [local, setLocal] = useState(defaultOpen ?? false)
  const shown = open ?? local
  return (
    <div className="o-collapsible">
      <button type="button" className="o-collapsible-head" onClick={() => { setLocal(!shown); onChange?.(!shown) }}>
        {shown ? '▾' : '▸'} {title} {description ? <span className="o-muted o-size-xs">{description}</span> : null}
      </button>
      {shown ? children : null}
    </div>
  )
}

export const Field = ({ children, label, description, error }: Children & { label: string; description?: string; error?: string; required?: boolean }) => (
  <div className="o-field"><span className="o-size-xs o-muted">{label}</span>{children}{description ? <span className="o-muted o-size-xs">{description}</span> : null}{error ? <span className="o-tone-danger o-size-xs">{error}</span> : null}</div>
)

export const Toolbar = ({ children, label }: Children & { label: string; gap?: Gap }) => <div role="toolbar" aria-label={label} className="o-box o-dir-row o-gap-xs">{children}</div>
export const Grid = ({ children, columns = 2 }: Children & { columns?: number }) => <div className="o-grid" style={{ gridTemplateColumns: `repeat(${columns}, 1fr)` }}>{children}</div>

type TabProps = Children & { value: string; label: string; icon?: string; disabled?: boolean }

/** 탭 하나. 그리는 일은 Tabs가 한다. */
export const Tab = ({ children }: TabProps) => <>{children}</>

/** Orca처럼 고른 탭의 내용만 그린다. */
export const Tabs = ({ children, label, value, defaultValue, onChange }: Children & {
  label: string; value?: string; defaultValue?: string; onChange?: (value: string) => void
}) => {
  const tabs = ReactChildren.toArray(children).filter((child): child is ReactElement<TabProps> => isValidElement(child))
  const [local, setLocal] = useState(defaultValue ?? tabs[0]?.props.value ?? '')
  const current = value ?? local
  const shown = tabs.find(tab => tab.props.value === current) ?? tabs[0]
  return (
    <div className="o-tabs">
      <div role="tablist" aria-label={label} className="o-tablist">
        {tabs.map(tab => (
          <button
            key={tab.props.value}
            type="button"
            role="tab"
            aria-selected={tab === shown}
            className={cx('o-tab', tab === shown && 'o-tab-current')}
            disabled={tab.props.disabled}
            onClick={() => { setLocal(tab.props.value); onChange?.(tab.props.value) }}
          >
            {tab.props.icon ? `${iconOf(tab.props.icon)} ` : ''}{tab.props.label}
          </button>
        ))}
      </div>
      <div role="tabpanel">{shown?.props.children}</div>
    </div>
  )
}

export const KeyValue = ({ label, items }: {
  label?: string; items: readonly { label: string; value: string | number | boolean | null; tone?: Tone; code?: boolean }[]
}) => (
  <dl className="o-kv" aria-label={label}>
    {items.map(item => (
      <div key={item.label} className="o-kv-row">
        <dt className="o-muted">{item.label}</dt>
        <dd className={cx(item.tone && `o-tone-${item.tone}`, item.code && 'o-code')}>{item.value === null ? '—' : String(item.value)}</dd>
      </div>
    ))}
  </dl>
)
