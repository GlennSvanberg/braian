import { LogOut, Moon, Sun } from 'lucide-react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from './components/ui/dropdown-menu'
import { copy } from './i18n'

export type Connection = 'online' | 'offline'

export function userInitial(user: { name?: string; email?: string; handle?: string }): string {
  const source = user.name || user.email || user.handle || ''
  const letter = source.trim().charAt(0)
  return letter ? letter.toUpperCase() : '?'
}

export function UserMenu({
  name,
  email,
  handle,
  role,
  connection,
  theme,
  onToggleTheme,
  onSignOut,
}: {
  name?: string
  email?: string
  handle?: string
  role: string
  connection: Connection
  theme: 'dark' | 'light'
  onToggleTheme: () => void
  onSignOut: () => void
}) {
  const identity = email || handle || name || ''
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" className="avatar-btn" data-testid="user-menu" aria-label={copy.account}>
          {userInitial({ name, email, handle })}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="user-menu-content" collisionPadding={8} data-testid="user-menu-content">
        <DropdownMenuLabel className="user-menu-head">
          <strong data-testid="user-menu-email">{identity}</strong>
          <span data-testid="user-menu-role">{role}</span>
          <span className="user-menu-status" data-testid="user-menu-status" data-online={connection === 'online'}>
            {connection === 'online' ? copy.connectedStatus : copy.offlineStatus}
          </span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem data-testid="user-menu-theme" onSelect={onToggleTheme}>
          {theme === 'dark'
            ? <Sun className="menu-icon" aria-hidden="true" size={14} />
            : <Moon className="menu-icon" aria-hidden="true" size={14} />}
          {theme === 'dark' ? copy.lightMode : copy.darkMode}
        </DropdownMenuItem>
        <DropdownMenuItem data-testid="user-menu-sign-out" onSelect={onSignOut}>
          <LogOut className="menu-icon" aria-hidden="true" size={14} />
          {copy.signOut}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
