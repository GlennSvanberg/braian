import { Command as CommandPrimitive } from 'cmdk'
import type { ComponentProps } from 'react'
import { cn } from '@/lib/utils'

export function Command({ className, ...props }: ComponentProps<typeof CommandPrimitive>) {
  return <CommandPrimitive className={cn('command', className)} {...props} />
}

export function CommandInput({ className, ...props }: ComponentProps<typeof CommandPrimitive.Input>) {
  return <CommandPrimitive.Input className={cn(className)} {...props} />
}

export function CommandList({ className, ...props }: ComponentProps<typeof CommandPrimitive.List>) {
  return <CommandPrimitive.List className={cn('palette-list', className)} {...props} />
}

export function CommandEmpty({ ...props }: ComponentProps<typeof CommandPrimitive.Empty>) {
  return <CommandPrimitive.Empty className="palette-empty" {...props} />
}

export function CommandGroup({ className, ...props }: ComponentProps<typeof CommandPrimitive.Group>) {
  return <CommandPrimitive.Group className={cn('palette-group', className)} {...props} />
}

export function CommandItem({ className, ...props }: ComponentProps<typeof CommandPrimitive.Item>) {
  return <CommandPrimitive.Item className={cn('hit', className)} {...props} />
}

export function CommandSeparator(props: ComponentProps<typeof CommandPrimitive.Separator>) {
  return <CommandPrimitive.Separator className="palette-sep" {...props} />
}
