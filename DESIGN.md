# Braian – designspråk

Braian ska kännas som ett snabbt, tätt proffsverktyg med värme, inte som en adminpanel. Förebilden är **P360 Client v4** (`C:\git\p360-gpui-v4`). Vi följer *inte* Fiwes produktmall (fiwe-product-ui); Braian har ett eget uttryck.

## Fem ord som fäller beslut

**Snabb, varm, exakt, lugn, verktygsmässig.**

- *Snabb*: allt nås med tangentbordet, inga onödiga klick, inga spinners som kan undvikas.
- *Varm*: djupt marinblått i stället för grått, mjuka hörn, en lila accent som lever.
- *Exakt*: tabulära siffror, raka linjer i tabeller, inga dekorationer som stjäl data.
- *Lugn*: en accentfärg. Statusfärger bara när de betyder något.
- *Verktygsmässig*: kompakt täthet, dämpade etiketter, starka värden.

## Språk i gränssnittet

**All UI copy is English.** This rule overrides any Swedish example in this document and in the product brief. It covers Ctrl+K groups, the last palette row (`Ask AI: …`), AI mode buttons (Cancel / Confirm), agent step texts and answers (the model is instructed to reply in English), the `Next time: <shortcut>` toast, error messages, activity log sentences, workspace templates, labels, and empty states. Catalog values and attribute labels per language (sv/en/de) stay in the data. Demo seed values are not translated to match the English chrome.

## Grund

- **shadcn/ui** (Radix + Tailwind) som komponentgrund. Komponenterna kopieras in och stylas med tokens nedan; standardutseendet från shadcn ska inte synas.
- `cmdk` för Ctrl+K, `sonner` för toasts, `lucide-react` för ikoner. Arbetsytans delning är egen flex (n-ära förhållanden och en 8 px träffyta). Paketet `react-resizable-panels` finns kvar installerat men används inte av trädet.
- Inga ad hoc-färger eller -radier i komponenter. Allt går via CSS-variabler.

## Färg (från P360 Client v4)

| Token | Mörkt | Ljust | Används till |
|---|---|---|---|
| `--bg` | `#0B0F1A` | `#F3F4F6` | Botten bakom panelerna |
| `--panel` | `#111827` | `#FFFFFF` | Paneler |
| `--raised` | `#182132` | `#F8F9FB` | Fält, kort, menyer |
| `--surface` | `#1E293B` | `#EEF0F4` | Tabellhuvud, flikrad |
| `--hover` | `#263247` | `#E3E6EC` | Hover |
| `--border` | `#3D4A60` | `#CDD2DB` | Starka kanter (sparsamt) |
| `--text` | `#F5F7FA` | `#111827` | Värden |
| `--text-2` | `#C3CAD6` | `#374151` | Sekundär text |
| `--muted` | `#8B95A7` | `#6B7280` | Etiketter, metadata |
| `--accent` | `#9A4DFF` | `#7C3AED` | Fokus, markering, aktiv flik, agent |
| `--accent-hover` | `#B27AFF` | `#6D28D9` | Hover på accent |
| `--accent-dim` | `#9A4DFF2E` | `#7C3AED1C` | Markerad rad, chip-bakgrund |
| `--info` | `#38BDF8` | `#0277B5` | Info |
| `--ok` | `#47FF9A` | `#13804F` | Klart, giltigt |
| `--warn` | `#F6C445` | `#A66A00` | Varning, utkast |
| `--danger` | `#FB7185` | `#C0233E` | Fel, ta bort |

Regler:
- Mörkt läge är standard. Båda lägena ska vara lika genomarbetade.
- Accenten används för *ett* syfte per yta: det som är aktivt eller i fokus.
- Ren grå (`#888` etc.) är förbjuden; neutralerna har blåton.
- Kanter mellan paneler görs med luft och ton, inte med linjer. Linjer bara inuti tabeller.

## Form

- Radie: `--r-sm: 4px` (kbd, checkbox), `--r-md: 6px` (knappar, fält, menyrader), `--r-lg: 8px` (paneler, kort, popovers), `--r-xl: 12px` (dialoger, paletten), `full` för chips och pills.
- Paneler flyter på `--bg` med 6 px mellanrum, aldrig kant-i-kant.
- Popovers, menyer och paletten har `shadow-md` och en 1 px kant i `--border`.
- I mörkt läge får upphöjda ytor en svag ljus kant upptill (`inset 0 1px 0 #ffffff0a`).

## Typografi

- **Inter** (variabel), med `font-feature-settings: "cv11", "ss01"`. Fallback `Segoe UI, system-ui`.
- Monospace: **JetBrains Mono** för id, kod och kbd.
- `font-variant-numeric: tabular-nums` i tabeller och siffror.
- Skala (px): 11 metadata/kbd, 12 tabell och etiketter, 13 brödtext och fält, 15 panelrubrik, 18 objektrubrik, 22 sidrubrik.
- Vikt: 400 och 500. 600 bara för objektrubrik. Aldrig 700.

## Täthet och rytm

- 4 px grid. Tabellrad 28 px, fält 30 px, knapp 28 px, flik 30 px.
- Formulär läses som kort: etikett i `--muted` till vänster, värde i ett mjukt fält (`--raised`, `--r-md`) till höger.
- Flerspråkiga värden visas som små kort med språket som etikett (som i v4).
- Chips: `full`-radie, `--accent-dim` bakgrund, accentfärgad text, t.ex. kontext `← Item: Items · NF-TENT-2401`.
- Id visas bredvid namn i `--muted` och mindre storlek (som `Tents & shelters FRILUFT-TALTS`).

## Rörelse

- 120–180 ms, `cubic-bezier(0.2, 0, 0, 1)`. Ingen studs.
- En live-ändring blinkar fältet kort i `--accent-dim` (600 ms fade).
- Paletten och menyer tonar och glider in 4 px.
- Respektera `prefers-reduced-motion`.

## Tangentbordet först

- Pilar flyttar i tabell, träd, listor och formulär. Enter öppnar, Esc går tillbaka.
- Fokus är alltid synligt: 2 px ring i `--accent` med 2 px offset.
- Varje kommando visar sitt kortkommando i `kbd`-stil i meny och palett.

## Fönsterlayout

Förebild: P360 Client v4 (`src/layout.rs`, `src/ui/dock.rs`, `src/core.rs`). Trädet är inte binärt. Ett `split` har `dir` (`horizontal` sida vid sida, `vertical` staplat), `ratios` som summerar till 1 och `children`. En `tabs`-nod har `views` och `active` som index. Arbetsytan bär `focus` och `zoom`.

- Paneler flyter med 6 px mellanrum. Avdelaren är 6 px, med en osynlig 8 px träffyta och ett 2 px streck som blir accent vid hover. Minsta andel är 0.04 vid drag och 0.05 vid tangentbordsändring.
- Flikar är 28 px i en 32 px remsa. Klick fokuserar, mushjulet eller krysset stänger (`Ctrl+Alt+W`), högerklick öppnar vymenyn. En dragen flik visar en spökbild med titeln.
- Varje ruta är ett släppmål. Zonen följer pekaren: vänster om 22 %, höger om 78 %, över om 25 %, under om 75 %, annars flik. Förhandsvisningen ligger inuti rutan med 4 px marginal, accentkant och texten `Dock left`, `Dock right`, `Dock top`, `Dock bottom` eller `Add as tab`. Släpp på flikraden sätter index. En tunn kant runt arbetsytan dockar mot kanten med andel 0.3. Esc avbryter draget.
- `Ctrl+Alt`+pilar flyttar vyn (`move_view_left/right/up/down`). `Alt`+pilar fokuserar grannen. `Ctrl+Alt+Shift`+pilar ändrar storlek med 0.05. `Ctrl+Alt+Z` maximerar rutan (`zoom`). Samma namn finns i menyn, Ctrl+K och operationsregistret. Agentens `open_view` utan zon hamnar till höger. `ui.link` byter inte trädet.
- Layouten sparas per person och miljö (`default`) som ett dokument. Webben ritar localStorage först och hämtar sedan servern. En låst eller delad arbetsyta skrivs inte över: ändringen blir personens kopia. Återställning tar den kanoniska layouten. Live-strömmen rör inte trädet. Ett gammalt binärt träd (`direction`/`ratio`/`first`/`second`) migreras vid läsning.

## Logga

- `apps/web/public/brand/` innehåller märket från P360 Client v4: rutnät av rundade rutor i lila (`#9A4DFF`, `#BA82FF`, `#7A35DC`).
- `braian-mark.svg` för fast färg, `braian-mark-currentColor.svg` för att ärva textfärg, `braian-app-icon.svg` och PNG-storlekar för favicon och Tauri.
- I toppraden: märket i 16 px följt av **Braian** i 13 px, vikt 600.

## Gör inte

- Hårda fyrkantiga boxar och 1 px ramar runt allt.
- Systemgrått, standardblått, shadcns standardtema.
- Fet text för betoning, versaler i rubriker.
- Fler än en accentfärg på samma yta.
- Dekorativa gradienter och illustrationer i arbetsytor.

## Kontroll innan merge

- Ser skärmen ut som en syster till P360 Client v4 i både mörkt och ljust läge?
- Går allt att göra med tangentbordet?
- Används bara tokens, inga hårdkodade färger eller radier?

## Ctrl+K – en yta, ingen chatt

Förebild: `src/ui/cmdk.rs` och `src/ai.rs` i P360 Client v4. Agenten är aldrig ett eget fönster eller en chatt.

- **En panel** överst i mitten (640 px, `--r-xl`, `--raised`, skugga) för fyra saker: *On this object*, *Go to*, *Do*, *Ask AI*. Ctrl+K öppnar, Ctrl+J öppnar direkt i AI-läge.
- **Grupperad lista**: Recent, Best match, On selected object, Views & workspaces, Objects. Fast radhöjd 44 px (inget hoppar), ikon, titel med träffmarkering, grå undertext som säger vad som händer, kortkommando som `kbd` till höger.
- **Sista raden är alltid** *Ask AI: "the text"* i accentfärg. Tab går dit. Matchar inget hamnar markeringen där – panelen ligger kvar.
- **AI-läge i samma panel**: lila kant och AI-bricka. Ovanför listan: tidigare repliker (klick = redigera och kör om), en löpande progressrad, steg i klartext ("Opening a list of articles…", "Filtering on 'tält'…") med bock när klara, och svaret i en mening på engelska. Stäng-knapp synlig; Esc/stäng avslutar samtalet.
- **Agenten styr gränssnittet**: samma operationsregister som UI och MCP, inklusive UI-operationer (öppna vy/mall/arbetsyta, placera vänster/höger/ovan/under/ny flik, länka vyer, filtrera, gruppera, sortera, markera, visa/dölj/flytta kolumner, byta tema, exportera). Stegen strömmas via SSE. Modellen instrueras att svara på engelska.
- **Inget sparas av agenten**: ändringar blir förslag som förhandsgranskas *i panelen* (antal, första raderna, fel) med Cancel / Confirm. Strukturändringar visar konsekvensvyn.
- **Tangentbord**: ↑↓, PageUp/Down, Home/End. Enter kör. Ctrl+Enter eller Ctrl+. visar radens sekundära åtgärder (t.ex. öppna i delning till höger, kopiera id). → på en objektträff visar dess åtgärder. Backspace på tom rad eller Esc backar ett steg i nästlade val (brödsmula-chip).
- **Lär ut genvägar**: efter att en rad körts visas toast "Next time: Ctrl+…". Frecency styr ordningen.
- Fotrad 30 px: vad Enter gör just nu, samt "↵ run · Ctrl+↵ actions · Tab ask AI · Esc close".
- **Tom detalj**: när inget objekt är valt visar detaljrutan en kort ledtext och kortkommandon (pilar i tabellen, Enter öppnar, Ctrl+K), inte bara ordet Detail.
