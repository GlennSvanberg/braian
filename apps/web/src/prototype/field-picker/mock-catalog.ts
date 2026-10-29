/* english-check: allow — mock bilingual product copy for the field-picker prototype */
import type { FieldDef } from './qualifiers'

/** Article peer fields (Net volume with unit, GTIN). */
export const ARTICLE_FIELDS: FieldDef[] = [
  {
    id: 'net_volume',
    name: 'Net volume',
    typeLabel: 'Number',
    shape: 'unit',
    units: ['ml', 'l'],
  },
  {
    id: 'gtin',
    name: 'GTIN',
    typeLabel: 'Text',
    shape: 'text',
  },
]

/** Supplier peer fields. */
export const SUPPLIER_FIELDS: FieldDef[] = [
  { id: 'name', name: 'Name', typeLabel: 'Text', shape: 'text' },
  { id: 'country', name: 'Country', typeLabel: 'Text', shape: 'text' },
]

/** Product root fields for the prototype catalog. */
export const PRODUCT_FIELDS: FieldDef[] = [
  { id: 'sku', name: 'SKU', typeLabel: 'Text', shape: 'text' },
  { id: 'name', name: 'Name', typeLabel: 'Text', shape: 'text' },
  {
    id: 'description',
    name: 'Description',
    typeLabel: 'Text',
    shape: 'language',
    languages: ['en', 'sv'],
  },
  {
    id: 'allergens',
    name: 'Allergens',
    typeLabel: 'Value list',
    shape: 'value_list',
    valueListModes: ['label', 'code'],
  },
  {
    id: 'storage_temperature',
    name: 'Storage temperature',
    typeLabel: 'Range',
    shape: 'range',
    rangeParts: ['min', 'max'],
    rangeUnit: '°C',
  },
  {
    id: 'articles',
    name: 'Articles',
    typeLabel: 'Link',
    shape: 'link',
    link: { many: true, peerFields: ARTICLE_FIELDS, peerLabel: 'Articles' },
  },
  {
    id: 'supplier',
    name: 'Supplier',
    typeLabel: 'Link',
    shape: 'link',
    link: { many: false, peerFields: SUPPLIER_FIELDS, peerLabel: 'Supplier' },
  },
]

export type ArticleRow = {
  key: string
  gtin: string
  net_volume: { value: number; unit: 'ml' | 'l' }
}

export type SupplierRow = {
  key: string
  name: string
  country: string
}

export type ProductRow = {
  id: string
  sku: string
  name: string
  description: { en: string; sv: string }
  allergens: { code: string; label: string }
  storage_temperature: { min: number; max: number }
  articles: ArticleRow[]
  supplier: SupplierRow
}

/** About ten English food products with 1–3 articles each. */
export const MOCK_PRODUCTS: ProductRow[] = [
  {
    id: 'p1',
    sku: 'OAT-001',
    name: 'Organic oat drink',
    description: { en: 'Creamy oat drink for coffee', sv: 'Krämig havredryck till kaffe' },
    allergens: { code: 'GLUTEN', label: 'Gluten' },
    storage_temperature: { min: 2, max: 8 },
    articles: [
      { key: '1L', gtin: '7300001000011', net_volume: { value: 1, unit: 'l' } },
      { key: '250ml', gtin: '7300001000012', net_volume: { value: 250, unit: 'ml' } },
    ],
    supplier: { key: 'nordic-oats', name: 'Nordic Oats AB', country: 'Sweden' },
  },
  {
    id: 'p2',
    sku: 'MLK-002',
    name: 'Whole milk',
    description: { en: 'Fresh whole milk 3.5%', sv: 'Färsk helmjölk 3,5%' },
    allergens: { code: 'MILK', label: 'Milk' },
    storage_temperature: { min: 2, max: 6 },
    articles: [
      { key: '1L', gtin: '7300002000021', net_volume: { value: 1, unit: 'l' } },
      { key: '1.5L', gtin: '7300002000022', net_volume: { value: 1.5, unit: 'l' } },
      { key: '250ml', gtin: '7300002000023', net_volume: { value: 250, unit: 'ml' } },
    ],
    supplier: { key: 'farm-dairy', name: 'Farm Dairy Co', country: 'Denmark' },
  },
  {
    id: 'p3',
    sku: 'YGT-003',
    name: 'Greek yogurt',
    description: { en: 'Thick strained yogurt', sv: 'Tjock grekisk yoghurt' },
    allergens: { code: 'MILK', label: 'Milk' },
    storage_temperature: { min: 2, max: 5 },
    articles: [{ key: '450g', gtin: '7300003000031', net_volume: { value: 450, unit: 'ml' } }],
    supplier: { key: 'hellenic', name: 'Hellenic Foods', country: 'Greece' },
  },
  {
    id: 'p4',
    sku: 'JCE-004',
    name: 'Orange juice',
    description: { en: 'Not from concentrate', sv: 'Inte från koncentrat' },
    allergens: { code: 'NONE', label: 'None' },
    storage_temperature: { min: 2, max: 8 },
    articles: [
      { key: '1L', gtin: '7300004000041', net_volume: { value: 1, unit: 'l' } },
      { key: '330ml', gtin: '7300004000042', net_volume: { value: 330, unit: 'ml' } },
    ],
    supplier: { key: 'citrus-co', name: 'Citrus Co', country: 'Spain' },
  },
  {
    id: 'p5',
    sku: 'BRD-005',
    name: 'Sourdough bread',
    description: { en: 'Stone-baked sourdough loaf', sv: 'Stenbakat surdegsbröd' },
    allergens: { code: 'GLUTEN', label: 'Gluten' },
    storage_temperature: { min: 15, max: 22 },
    articles: [{ key: '800g', gtin: '7300005000051', net_volume: { value: 800, unit: 'ml' } }],
    supplier: { key: 'bakery-nord', name: 'Bakery Nord', country: 'Sweden' },
  },
  {
    id: 'p6',
    sku: 'PSTA-006',
    name: 'Penne pasta',
    description: { en: 'Durum wheat penne', sv: 'Durumvetepenne' },
    allergens: { code: 'GLUTEN', label: 'Gluten' },
    storage_temperature: { min: 10, max: 25 },
    articles: [
      { key: '500g', gtin: '7300006000061', net_volume: { value: 500, unit: 'ml' } },
      { key: '1kg', gtin: '7300006000062', net_volume: { value: 1000, unit: 'ml' } },
    ],
    supplier: { key: 'pasta-roma', name: 'Pasta Roma', country: 'Italy' },
  },
  {
    id: 'p7',
    sku: 'OLV-007',
    name: 'Extra virgin olive oil',
    description: { en: 'Cold-pressed olive oil', sv: 'Kallpressad olivolja' },
    allergens: { code: 'NONE', label: 'None' },
    storage_temperature: { min: 12, max: 20 },
    articles: [
      { key: '500ml', gtin: '7300007000071', net_volume: { value: 500, unit: 'ml' } },
      { key: '1L', gtin: '7300007000072', net_volume: { value: 1, unit: 'l' } },
      { key: '250ml', gtin: '7300007000073', net_volume: { value: 250, unit: 'ml' } },
    ],
    supplier: { key: 'olive-grove', name: 'Olive Grove Ltd', country: 'Portugal' },
  },
  {
    id: 'p8',
    sku: 'CHSE-008',
    name: 'Cheddar cheese',
    description: { en: 'Aged cheddar block', sv: 'Lagrad cheddar' },
    allergens: { code: 'MILK', label: 'Milk' },
    storage_temperature: { min: 2, max: 6 },
    articles: [
      { key: '200g', gtin: '7300008000081', net_volume: { value: 200, unit: 'ml' } },
      { key: '400g', gtin: '7300008000082', net_volume: { value: 400, unit: 'ml' } },
    ],
    supplier: { key: 'cheddar-vale', name: 'Cheddar Vale', country: 'United Kingdom' },
  },
  {
    id: 'p9',
    sku: 'NUT-009',
    name: 'Roasted almonds',
    description: { en: 'Lightly salted roasted almonds', sv: 'Lättsaltade rostade mandlar' },
    allergens: { code: 'NUTS', label: 'Nuts' },
    storage_temperature: { min: 10, max: 22 },
    articles: [{ key: '150g', gtin: '7300009000091', net_volume: { value: 150, unit: 'ml' } }],
    supplier: { key: 'nut-house', name: 'Nut House', country: 'USA' },
  },
  {
    id: 'p10',
    sku: 'SOY-010',
    name: 'Soy sauce',
    description: { en: 'Naturally brewed soy sauce', sv: 'Naturligt bryggd sojasås' },
    allergens: { code: 'SOY', label: 'Soy' },
    storage_temperature: { min: 10, max: 25 },
    articles: [
      { key: '150ml', gtin: '7300010000101', net_volume: { value: 150, unit: 'ml' } },
      { key: '500ml', gtin: '7300010000102', net_volume: { value: 500, unit: 'ml' } },
    ],
    supplier: { key: 'nippon-taste', name: 'Nippon Taste', country: 'Japan' },
  },
]

export function fieldById(fields: FieldDef[], id: string): FieldDef | undefined {
  return fields.find((field) => field.id === id)
}

/* english-check: end */
