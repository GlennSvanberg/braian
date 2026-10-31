import type { Json } from './api'

/** Palette progress lines. UI copy is English. */
export function stepText(op: string, args: Json = {}, typeName?: { singular?: string; plural?: string }): string {
  const query = text(args.q ?? args.text ?? args.filter)
  const plural = typeName?.plural?.trim()
  const singular = typeName?.singular?.trim()
  switch (op) {
    case 'read':
      return 'Reading the question…'
    case 'search':
    case 'list_objects':
      return query ? `Filtering on '${query}'…` : plural ? `Opening a list of ${plural.toLowerCase()}…` : 'Opening a list of objects…'
    case 'ui.filter':
      return `Filtering on '${query}'…`
    case 'ui.group':
      return 'Grouping the list…'
    case 'ui.sort':
      return 'Sorting the list…'
    case 'ui.open_workspace':
    case 'ui.open_template':
      return 'Opening the workspace…'
    case 'ui.open_changes':
      return 'Opening the changes…'
    case 'ui.open_view':
    case 'ui.place':
    case 'open_view':
      return 'Opening the view…'
    case 'move_view':
    case 'move_view_left':
    case 'move_view_right':
    case 'move_view_up':
    case 'move_view_down':
      return 'Moving the view…'
    case 'split':
      return 'Splitting the pane…'
    case 'resize':
      return 'Resizing the pane…'
    case 'swap':
      return 'Swapping the views…'
    case 'focus':
      return 'Focusing the pane…'
    case 'zoom':
      return 'Maximizing the pane…'
    case 'close_view':
      return 'Closing the view…'
    case 'reset_layout':
      return 'Resetting the layout…'
    case 'get_layout':
      return 'Reading the layout…'
    case 'ui.select':
    case 'get_object':
      return singular ? `Opening the ${singular}…` : 'Opening the object…'
    case 'ui.get_context':
      return 'Reading the screen…'
    case 'ui.get_selection':
      return 'Reading the selection…'
    case 'ui.select_rows':
      return 'Selecting the rows…'
    case 'ui.show_column':
    case 'ui.hide_column':
    case 'ui.move_column':
      return 'Changing the columns…'
    case 'ui.set_theme':
      return 'Switching theme…'
    case 'ui.export':
    case 'export':
      return 'Exporting…'
    case 'patch_object':
    case 'create_object':
    case 'mass_edit':
    case 'create_link':
    case 'rollback':
      return 'Preparing a proposal…'
    case 'list_assortments':
      return 'Listing selections…'
    case 'assortment.save':
      return 'Preparing a selection…'
    case 'model.list_attribute_definitions':
      return 'Listing attribute definitions…'
    case 'model.attribute_usage':
      return 'Reading attribute usage…'
    case 'model.command':
    case 'model.draft':
    case 'model.move_class':
    case 'model.add_attribute':
    case 'model.remove_attribute':
    case 'model.reorder_attributes':
    case 'model.create_attribute':
    case 'model.attach_attribute':
    case 'model.detach_attribute':
    case 'model.update_attribute':
    case 'model.create_class':
    case 'model.create_group':
    case 'model.rename_type_key':
    case 'model.rename_attribute_key':
    case 'model.rename_structure_key':
    case 'model.rename_group_key':
    case 'model.rename_value_list_key':
    case 'model.rename_unit_family_key':
    case 'model.rename_group':
    case 'model.move_group':
    case 'model.duplicate_group':
    case 'model.delete_group':
    case 'model.set_parent':
      return 'Preparing a data model proposal…'
    default:
      return `Running ${op}…`
  }
}

function text(value: unknown) {
  return typeof value === 'string' ? value : ''
}
