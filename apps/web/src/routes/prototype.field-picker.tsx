import { createFileRoute } from '@tanstack/react-router'
import { FieldPickerPrototypePage } from '../prototype/field-picker/page'

export const Route = createFileRoute('/prototype/field-picker')({
  component: FieldPickerPrototypePage,
})
