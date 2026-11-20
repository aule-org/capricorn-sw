---
version: alpha
name: Aule demo (playground)
description: Paper and ink with a single live colour, copper. Every value is taken from apps/playground/public/style.css; none is new.
colors:
  paper: "#f5f2ea"          # page background: "paper and ink", like the device's screen
  paper-deep: "#ece8dc"     # bg-2: button hover and the flat shadow of cards
  surface: "#fbfaf6"        # cards, fields and buttons: a warm white on the paper
  primary: "#141414"        # the ink: text, strong borders and the accent of buttons (--accent is this same value)
  on-ink: "#fbfaf6"         # accent-ink: text on an ink background (same as surface)
  muted: "#6b6b6b"          # secondary text and notes
  line: "#c9c3b4"           # soft borders of fields, cards and dividers
  copper: "#9a5b2e"         # "the one colour: worked copper, for what is alive on the page"
  copper-soft: "#e9d8c4"    # halo of the tutor's orb; copper at low volume
  danger: "#8f1d14"         # errors and destructive actions
  tile: "#14130f"           # background of the video and tutor tiles
  tile-ink: "#f6f4ef"       # text on the dark tiles
  board: "#ffffff"          # what the tutor shows: "always black on white, like the device's screen"
  device-frame: "#1b1b1b"   # the 14px frame of the device screen (800 x 480)
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, system-ui, Roboto, Helvetica Neue, Arial, sans-serif"
    fontSize: 17px          # page text; the system stack avoids loading external fonts
    fontWeight: 400
    lineHeight: 1.5
  title:
    fontFamily: "Source Serif 4"
    fontSize: 1.9rem        # h1: "the tutor's words and the titles are set in a serif made for reading"
    fontWeight: 560
    letterSpacing: -0.01em
  heading:
    fontFamily: "Source Serif 4"
    fontSize: 1.125rem      # h2 of the lobby; same weight and tracking as the title
    fontWeight: 560
    letterSpacing: -0.01em
  clock:
    fontFamily: "Source Serif 4"
    fontSize: 1.05rem       # the session clock, in the serif with tabular figures
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, system-ui, Roboto, Helvetica Neue, Arial, sans-serif"
    fontSize: 0.875rem      # field labels and supporting text
    fontWeight: 600
  note:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, system-ui, Roboto, Helvetica Neue, Arial, sans-serif"
    fontSize: 0.8125rem     # notes and small captions (no reason is written in style.css; it is the size in use)
    fontWeight: 400
rounded:
  xs: 3px                   # the microphone level bar
  sm: 6px                   # the caption over the video
  md: 8px                   # the error message
  lg: 12px                  # fields, video tiles and the supervisor panel: the most used radius (6 times)
  xl: 16px                  # the tutor's board
  2xl: 22px                 # the form card
  full: 999px               # buttons: pill shape
spacing:
  xs: 8px                   # the most repeated gap (11 times): controls, lists, fields
  sm: 12px                  # gap between groups and top margin of controls (3 and 3 times)
  md: 16px                  # gap between the fields of a form
  lg: 20px                  # gap of the session stage and top margin of blocks (3 times)
  xl: 24px                  # padding of the form card (2 times)
  2xl: 28px                 # top margin of the form, the stage and the lobby (3 times)
components:
  button:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    typography: "{typography.label}"
    rounded: "{rounded.full}"
    padding: 9px
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.on-ink}"
    rounded: "{rounded.full}"
    padding: 9px
  button-hover:
    backgroundColor: "{colors.paper-deep}"
    textColor: "{colors.primary}"
  button-talk-active:
    backgroundColor: "{colors.copper}"
    textColor: "{colors.surface}"
    rounded: "{rounded.full}"
  orb:
    backgroundColor: "{colors.copper-soft}"
    rounded: "{rounded.full}"
  note:
    textColor: "{colors.muted}"
    typography: "{typography.note}"
  device-screen:
    backgroundColor: "{colors.device-frame}"
    textColor: "{colors.board}"
    rounded: 10px
  button-primary-hover:
    backgroundColor: "#000000"
    textColor: "{colors.on-ink}"
  button-danger:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.danger}"
    rounded: "{rounded.full}"
  field:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    rounded: "{rounded.lg}"
    padding: 11px
  card-form:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    rounded: "{rounded.2xl}"
    padding: "{spacing.xl}"
  video-tile:
    backgroundColor: "{colors.tile}"
    textColor: "{colors.tile-ink}"
    rounded: "{rounded.lg}"
  board:
    backgroundColor: "{colors.board}"
    textColor: "{colors.primary}"
    rounded: "{rounded.xl}"
  message-tutor:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.primary}"
    rounded: 14px
  message-you:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.surface}"
    rounded: 14px
  error:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.danger}"
    rounded: "{rounded.md}"
---

## Overview

The demo looks like the device's screen: warm paper, near-black ink and a single live colour, copper, kept for what is active (the tutor's orb, the talk button). Everything else is neutral. Titles and the tutor's words are set in a serif; the interface uses the system typeface.

This file describes what already exists in `public/style.css`. It defines no new values: when a value changes in the CSS, it changes here in the same commit.

Figma: <https://www.figma.com/design/yOAIYJNl5nZ7NgWtESN0Fo> (the "Login v4" pages).

## Colors

Paper (`paper`, `paper-deep`, `surface`), ink (`primary`, which is the ink, `muted`, `line`) and one accent: `copper` with its soft tone `copper-soft`. `danger` is only for errors and destructive actions. `tile` and `tile-ink` are the dark video tiles. The tutor's board (`board`) is always black on white, like the device's screen.

Today `--accent` in the CSS is the ink itself (`#141414`), not copper: primary buttons are black. Copper appears only where something is alive.

## Typography

Source Serif 4 (served by the playground's own server, no external fonts) for titles, headings, the clock and the tutor's words. The system typeface for everything else. Body text at 17px with a line height of 1.5.

The CSS also sets a small uppercase caption (0.7rem, 0.08em tracking) above each message in the conversation. It is what exists today and it is not a token because it is not a pattern to repeat.

## Layout

One column of at most 880px with 16px side margins. The session stage is two columns (3fr and 2fr, the right one at least 320px) with 20px between them. The spacing scale (`xs` to `2xl`) comes from the loose values that repeat most in the CSS: 8, 12, 16, 20, 24 and 28px. Other isolated values (for example 2, 6, 10 and 14px) stay in the CSS and are not tokens.

## Elevation & Depth

No soft shadows. The form card has a 2px ink border and a flat shadow offset by 6px in `paper-deep`; the board, 5px. It is the "paper pinned to the desk" device.

## Shapes

Radii from 3 to 22px, and a pill for buttons (see `rounded`). The most common is 12px. The form card is the roundest (22px); the board is 16px.

## Components

The components above summarise the ones that already exist. Button padding (9px 18px) and field padding (11px 14px) have two values in the CSS; only the vertical one is recorded here because the format takes a single dimension.

## Do's and Don'ts

- Do use copper only for what is alive; keep everything else neutral.
- Do respect `prefers-reduced-motion`, as the CSS already does for the orb and the device board.
- Don't load fonts, scripts or images from other domains.
- Don't add colours, radii or spacings that are not in this file without changing the CSS and this file together.
- Don't put data about children or families in screenshots, examples or designs.

## Interaction pattern

Not built yet: the demo does not implement it today. It is what has been decided for the entry flows (login, first time), and it is drawn in Figma, in the "Login v4" pages.

- **One question per screen**, like a conversational form, in Aule's own look: paper, ink, the serif for the question and copper only for what is active.
- **A 250 ms transition** between questions: the screen leaves sliding and fading out, and the next one enters the same way. If the system asks for reduced motion (`prefers-reduced-motion: reduce`), there is no transition: the change is immediate.
- **Enter advances** to the next question. Focus moves to the new question, with a visible focus ring (3px in ink, like the one buttons and fields already have).
