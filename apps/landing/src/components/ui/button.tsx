import { cva, type VariantProps } from 'class-variance-authority'
import type { ComponentProps } from 'react'
import { cn } from '@/lib/utils'

const buttonVariants = cva('btn', {
  variants: {
    variant: {
      primary: 'btn-primary',
      raised: 'btn-raised',
      ghost: 'btn-ghost',
    },
    size: {
      default: '',
      lg: 'btn-lg',
    },
  },
  defaultVariants: { variant: 'primary', size: 'default' },
})

type Variant = VariantProps<typeof buttonVariants>

export function Button({
  variant,
  size,
  className,
  ...props
}: ComponentProps<'button'> & Variant) {
  return <button className={cn(buttonVariants({ variant, size }), className)} {...props} />
}

export function ButtonLink({
  variant,
  size,
  className,
  ...props
}: ComponentProps<'a'> & Variant) {
  return <a className={cn(buttonVariants({ variant, size }), className)} {...props} />
}

export { buttonVariants }
