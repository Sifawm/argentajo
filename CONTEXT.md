# Argentajo

Nests jewellery motifs onto silver Sheets by their real contour, so the most pieces fit into the least silver, and prints an exact-scale Template to glue on the silver and cut with a saw. Terms are given in English (as in the code) with the Spanish UI word in brackets.

## Library

**Jeweller** (joyero):
The person using the app: prepares Pieces, sets up Jobs and cuts the silver.
_Avoid_: User, customer

**Motif** (motivo):
The drawing in an imported PNG, JPG or SVG, before it is cleaned.
_Avoid_: Design, artwork

**Library** (Biblioteca):
The Jeweller's saved Pieces, kept in app data, that Jobs are made from.
_Avoid_: Catalogue, gallery

**Piece** (pieza):
A cleaned Motif in the Library: background removed, cropped to its mask, with one contour per Part.
_Avoid_: Shape, item, motif (once cleaned)

**Part**:
One separate blob of a Piece. Parts are cut separately but nested together: all of them or none.
_Avoid_: Fragment, island

**Size** (tamaño):
A named real size of a Piece: a name the Jeweller chooses plus one side in mm (width _or_ height), the other side following the proportions. Cleaning, Edits, rotation/mirror and the contour are shared by all of a Piece's Sizes; ppi and the low-resolution warning are per Size.
_Avoid_: Variant, scale, real size (the old single-size term)

**Default size** (tamaño predeterminado):
The first of a Piece's Sizes: the one a Piece is added to a Job with and shown on its Library card. Every Piece has at least one Size; a new or migrated one gets "Normal".

**Tolerance** (tolerancia del fondo):
How far from white a pixel may be and still count as background. Ignored when the image already has transparency.

**Edits** (pincel / borrador):
The Jeweller's manual corrections to the automatic mask: the brush forces pixels into the Piece, the eraser out of it. They survive Tolerance changes.
_Avoid_: Mask edits, paint

**Split** (dividir):
Saving an image as one Piece per Part, at the scale the whole image has. A split Piece keeps no source and reopens from its own cleaned image.

**Low resolution** (resolución baja):
A Size whose effective resolution is under 150 ppi: the print gets blurry and the contour inaccurate.

## Job

**Job** (trabajo):
One nesting setup: Paper, Sheets, spacing, time limit and Items.
_Avoid_: Project, order

**Item** (línea / pieza del trabajo):
A Piece at one of its Sizes in a Job, with a Quantity. A Job never holds two Items with the same Piece and Size; the same Piece may appear once per Size.
_Avoid_: Line, row, entry

**Quantity** (cantidad):
How many copies of an Item to place: **Fixed** (this many, the rest Missing) or **Fill** (rellenar: as many as fit in the silver the Fixed quantities leave; never Missing).

**Sheet** (lámina de plata):
A rectangle of silver, width × height in mm as it lies on the Paper. A Job's Sheets are filled in order.
_Avoid_: Plate, board, blank

**My sheets** (mis láminas):
Sheet dimensions the Jeweller saved for reuse across Jobs.

**Paper** (papel):
The page the Template is printed on: a preset (A3, A4, A5, Letter) or custom, portrait or landscape, with a **Printer margin** (margen de impresora) left unprinted.

**Separation** (separación):
Minimum distance between two placed Parts, for the saw.

**Edge margin** (margen al borde):
Minimum distance between a placed Part and its Sheet's edge.

**Nesting** (cálculo / encaje):
The search for a Layout, bounded by the Job's **Time limit** (tiempo máximo). It reports improved Layouts as it goes and can be stopped early.
_Avoid_: Packing, optimisation

**Layout** (distribución):
A Nesting result: where each copy of each Item lies on which Sheet, with the copies **Placed** (colocadas) and **Missing** (no cupieron) per Item, and the **Utilisation** (plata aprovechada): placed area over Sheet area.
_Avoid_: Solution, arrangement

**Exclude** (excluir del resultado):
Taking an Item out of the Job from the Layout and nesting what's left again.

## Output

**Template** (plantilla):
The Layout printed at exact scale, one or more pages, glued on the silver to cut along.
_Avoid_: Print, stencil

**Verification square** (cuadrado de verificación):
A black 1 × 1 cm square on every page, to check with a ruler that it printed at 100 %.

**Sheet outline** (contorno de las láminas):
An optional line round each Sheet on the Template, to know where to glue it.

**Silhouette** (silueta de corte):
An optional line along each Part's contour on the Template, as a guide for the saw.

## Files and preferences

**Job file** (archivo de trabajo):
A saved Job: its settings, its own copy of each Item's Piece at the Item's Size (with the Size's name), the output options and the latest Layout if it was nested for those settings. Reopened, it does not follow later Library changes, and its Items' Sizes are fixed.

**Job pieces**:
The Piece copies of the Job file opened last, which its Items refer to instead of the Library.

**Preferences** (preferencias):
The Jeweller's **Unit** (mm, cm or px at 96 per inch; geometry is always stored in mm), language and the values every new Job starts with.

## Relationships

- A **Piece** has one or more **Parts** and one or more **Sizes**; the first Size is the **Default size**.
- A **Job** has one or more **Sheets** on one **Paper** and one or more **Items**; each Item is one **Piece** at one of its **Sizes**.
- **Nesting** a Job gives a **Layout**, printed as a **Template**.
- Changing a Size's measure in the Library changes the Items that use it; deleting a Size or a Piece drops its Items from the Job. A **Job file** is not affected.

## Flagged ambiguities

- "Tamaño" in the UI also labels the Paper preset and the brush; in this glossary **Size** is only a Piece's named real size. A Sheet's or Paper's width × height are its _dimensions_ (the code's `Size` type in `src/job.ts` is just that).
- "Plantilla" in conversation sometimes means the Job or its Layout; **Template** is only the printed output.
