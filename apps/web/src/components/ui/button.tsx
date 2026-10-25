import { Slot } from '@radix-ui/react-slot'
import { cva, type VariantProps } from 'class-variance-authority'
import type { ComponentProps } from 'react'
import { cn } from '@/lib/utils'

const buttonVariants = cva(
  'btn inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-[var(--r-md)] text-[13px] font-medium transition-[background-color,color] duration-150 ease-[cubic-bezier(0.2,0,0,1)] disabled:pointer-events-none disabled:opacity-40 [&_svg]:pointer-events-none [&_svg]:size-3.5',
  {
    variants: {
      variant: {
        default: 'btn-accent',
        raised: 'btn-raised',
        ghost: 'btn-ghost',
        danger: 'btn-danger',
      },
      size: {
        default: 'h-7 px-2.5',
        icon: 'size-7',
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
)

export function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: ComponentProps<'button'> & VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : 'button'
  return <Comp className={cn(buttonVariants({ variant, size }), className)} {...props} />
}

export { buttonVariants }
