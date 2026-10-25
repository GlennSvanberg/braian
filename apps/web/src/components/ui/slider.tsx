import * as SliderPrimitive from '@radix-ui/react-slider'
import type { ComponentProps } from 'react'
import { cn } from '@/lib/utils'

export function Slider({ className, ...props }: ComponentProps<typeof SliderPrimitive.Root>) {
  return (
    <SliderPrimitive.Root className={cn('slider', className)} {...props}>
      <SliderPrimitive.Track className="slider-track">
        <SliderPrimitive.Range className="slider-range" />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb className="slider-thumb" />
    </SliderPrimitive.Root>
  )
}
