# Entrega: login del padre más simple (aule-sw, `apps/playground`)

Rama `claude/login-padre-simple` (subida a `meet-aule/aule-sw`, sin PR, `main` intacto). Solo toqué `apps/playground`; la plataforma no se tocó.

## Commits
- `f817975` feat(playground): sign in with a mailed code, and start with memory off — `src/server.ts`, `src/session.ts`, `src/students.ts`, `test/signin.test.ts`, `test/students.test.ts`
- `c03e083` feat(playground): mailed-code sign-in on the page, and a one-field student sign-up — `public/index.html`, `signin.js`, `students.js`, `style.css`

## Lo que hay que decidir (importante)
**El alta por código exige el nombre del adulto** (`signin.rs:80-` `enter`: «name is needed to create the account», y `verify` lo pasa por `name`). No cambié la plataforma. Lo resolví solo en el playground: la primera vez, al pulsar «Entrar» con el código correcto, la plataforma responde 400 «name needed»; el playground lo traduce a `name_needed`, la página muestra un campo «¿Cómo te llamamos?» y el mismo código sigue valiendo (la transacción se revierte, no cuenta intento). Para quien ya tiene cuenta no aparece nada. Es **un campo extra solo la primera vez**. Si Cesar quiere cero campos, hace falta un cambio en la plataforma (p. ej. nombre opcional con el correo como nombre provisional): dime y lo encargas.

## Qué cambió
1. **Correo y código.** Rutas nuevas `POST /api/signin/start` (`{email, language}` → `/v1/auth/start`) y `POST /api/signin/verify` (`{email, code, name?, language}` → `/v1/auth/verify`), con la misma cookie `aule_guardian` que Google (`signedInCookie`). El código acepta espacios o guiones al pegar («123 456»); debe ser de 6 dígitos o se rechaza sin llamar a la plataforma. Errores: 403 `code_refused`, 400 `name_needed` / `bad_request`, 503 `mail_unavailable`. En la pantalla «Entra a tu sesión»: botón de Google (el de hoy) + «Continuar con tu correo» → correo + «Enviarme un código» → código de 6 dígitos (`autocomplete="one-time-code"`, admite pegar) + «Entrar» (+ «Usar otro correo»).
2. **Alta del alumno.** Campo «Nombre de usuario del alumno» con la ayuda «Un apodo inventado. No uses su nombre real.» y la casilla de consentimiento como estaba. Grado, relación, asignaturas, intereses, tono, nota, idioma y los dos interruptores de «Lo que guardamos» (memoria y transcripciones) solo salen en «Configurar». Al crear se envía `language: es`, `remembers: false`, `keeps_transcripts: false`.
3. **Memoria apagada.** `profileFrom` ahora da `remembers: body.remembers === true` (antes `!== false`), y el interruptor del editor arranca sin marcar. El resto del consentimiento no cambió (voz sí, transcripciones no, versión de política igual).
4. Decisión mía: en el alta también quedan ocultas las casillas de memoria y de transcripciones (el encargo dice «un campo y una casilla»); se activan en «Configurar». Si quieres la de memoria visible en el alta, apagada, es una línea.

## Pruebas (salida real)
- `pnpm --filter @aule/playground typecheck` → `tsc --noEmit` sin errores.
- `pnpm --filter @aule/playground test` → `tests 57 · pass 57 · fail 0`.
- Nuevas en `test/signin.test.ts` (plataforma simulada con el doble que ya había): alta con solo usuario y casilla (memoria `granted:false`); sin la casilla → 400; correo → código → entra con cookie; código equivocado o mal formado (`654321`, `12345`, `abcdef`, vacío, número) → 403 sin cookie; correo nuevo → `name_needed`, mismo código con nombre → entra; `mail_unavailable`; Google sigue igual (las pruebas de Google no se tocaron y pasan). `test/students.test.ts`: la memoria arranca apagada y solo se enciende con `remembers: true`.
- Nota de entorno: no había `pnpm`; usé `npx pnpm@12.20.0` (la 12.21.0 del `packageManager` aún no pasa la política de edad de npm). `pnpm install --frozen-lockfile` salió bien.
- **Prueba en navegador real** (Chrome sin ventana, plataforma simulada, usuario inventado `tigre_azul`, no el servicio real): botón de correo → correo → código malo («Ese código no es válido…») → código bueno sin nombre («Es tu primera vez…», aparece el campo) → con nombre entra y abre «Nuevo alumno» → alta con usuario + casilla → aparece en la lista. Capturas vistas a ojo (1-entrar, 3-primera-vez, 4-alta): se leen bien y no se desbordan. El escrito a la plataforma simulada confirmó memoria `granted:false`.
- **No probado:** el envío real de correo ni Google real con esta rama; la página en móvil (solo ancho 700 px); el Rust no se corrió (no se tocó).

## Antes y después (desde que llega la familia hasta «Entrar ahora»)
Por Google, familia nueva. Contado en el DOM del navegador:
- **Antes:** 3 pantallas (Entrar → Nuevo alumno → «¿Quién va a estudiar hoy?» con «Continuar»). El alta tenía **28 controles** (33 si la plataforma guarda asignaturas): nombre, grado, relación, 16 intereses + otro + Agregar, 4 tonos, nota, idioma, memoria, transcripciones, consentimiento, Guardar, Volver. Campos obligatorios: nombre y casilla.
- **Después:** 3 pantallas igual, pero el alta tiene **4 controles** (usuario, casilla, Guardar, Volver). Obligatorios: usuario y casilla.
- Por correo, familia nueva: Entrar → correo → código (+ nombre del adulto la primera vez) → alta → lista. Una pantalla más que Google; es la parte de «quitar» que no pude resolver sin tocar la plataforma. No hice nada de contraseñas.
- Sigue en pie: tras crear al primer alumno hay que pulsar «Continuar» en la lista (no entra directo). No lo cambié porque el encargo no lo pedía.

## No hice / límites
- Camino del alumno y cuenta pendiente: no (esperan a Cesar).
- La pantalla de entrada solo se muestra si la plataforma tiene Google configurado (`google_client_id`), como hoy; un despliegue con correo pero sin Google no vería el botón de correo. Cambiarlo requiere añadir un campo a `/api/account` y tocar las pruebas que fijan esa respuesta.
- Los textos de la página están solo en español, como todo el playground hoy; el correo del código sale en `es` (la plataforma ya lo tiene en `en` también). No hay selector de idioma que lo cambie.
- Posible choque con Gonzalo: toqué `server.ts`, `session.ts`, `students.ts`, `index.html`, `signin.js`, `students.js`, `style.css` y dos archivos de prueba. Todo dentro de `apps/playground`.
- No actualicé `~/aule-server/handoffs/estado.md` (lo lleva el arquitecto). No hice commit del `README` del playground (no menciona el login).

## Ronda 2 (hallazgos de aule-qa)

Misma rama. Commits, en orden (todos en `apps/playground`):
- `a4cdbc1` el correo ya no depende de Google · `0309cdf` se sirven `mail.js` y `texts.js` (a4cdbc1 solo no cargaba la página: faltaban esas dos rutas; ahora una prueba revisa que todo `import` de la página se sirva) · `b69cced` «Usar otro correo» olvida el nombre y el nombre solo va si se pidió · `da294bf` textos en español e inglés y nada que prometa un envío · `ac3ba23` la respuesta con sesión también trae `mail`, para que el acceso del equipo no quede abierto · `09f0be6` mueve la nota del código bajo su campo (solo orden en pantalla) · el commit de la entrega es este archivo.

1. **Correo sin Google.** `/api/account` ahora trae `mail: true` siempre que hay plataforma, y la página muestra cada método por su cuenta (`methods()` en `public/mail.js`). Sin Google, el formulario del correo sale abierto. **Límite:** la plataforma no dice si tiene correo configurado (solo expone el `client_id` de Google), así que el playground lo ofrece siempre y, si no hay correo, la plataforma responde 503 y la página dice «Ahora no podemos enviar códigos por correo». Saber de antemano si hay correo pide un campo en la plataforma; no lo toqué.
2. **Sin afirmar el envío.** Se quitó «te lo enviamos por correo». Al pedir el código la página muestra: «Si el correo es correcto, te llegará un código en un minuto. Si no llega, revisa la carpeta de spam o vuelve a intentarlo en unos minutos.» (y su par en inglés). Dice lo mismo con cuota agotada, cuenta nueva o ya existente. La prueba fija el texto y que no diga «te enviamos».
3. **Otro correo.** `clearName` vacía el nombre y esconde el campo al pedir otro código y al pulsar «Usar otro correo»; `verifyBody` solo manda `name` si el campo está visible. En el navegador: nombre escrito → «Usar otro correo» → el verify siguiente va sin `name`; solo vuelve a ir cuando la plataforma lo pide otra vez.
4. **Español e inglés.** `public/texts.js` tiene todos los textos nuevos (entrada, correo, errores, alta con un campo) en los dos idiomas; el HTML conserva el español (se lee igual sin script) y `applyTexts` pone el idioma. El idioma es el del navegador (`es` si no es ni es ni en) y se manda en `start` y en `verify`. Pruebas: mismas claves en ambos idiomas, ninguna vacía, todo `data-t` del HTML existe y su español coincide con el HTML. **No hay selector de idioma** y el resto de la página (lista de alumnos, antesala, sesión) sigue solo en español, como antes: lo no tocado por esta rama no se tradujo. El alumno se sigue creando con `language: es`.
5. **Antes y después, con la antesala** (Chrome sin ventana, plataforma simulada, usuario inventado `tigre_azul`; antes = `32df820`, después = esta rama). Controles visibles por pantalla:
   - Google, familia nueva — antes: alta **28** → lista **5** → antesala **4**; después: alta **4** → lista **5** → antesala **4**. (Google se probó con la misma cookie que deja su botón; el botón en sí necesita los servidores de Google.)
   - Correo, familia nueva — después: entrar **1** (Google) → correo **2** → código **3** → primera vez **4** → alta **4** → lista **5** → antesala **4**. Sin Google: entrar **2** (ya sale el formulario del correo).
   - La antesala son 4 controles en los tres recorridos: «Micrófono», «Cámara», «Entrar ahora», «Volver». Después del primer alumno sigue haciendo falta «Continuar» en la lista.
   - Con el navegador en inglés, el mismo recorrido por correo sale en inglés y `start` y `verify` llevan `language: en`; en español, `es`.
   - Un error mío que la prueba del navegador encontró: con Google apagado, la respuesta con sesión no traía `mail` y el acceso del equipo salía abierto en el alta (8 controles en vez de 4). Corregido en `ac3ba23` con prueba.

**Para Gonzalo (no lo arreglé, como pediste):** el límite de envíos de códigos se reinicia al entrar (`apps/platform/src/signin.rs:87, 158`), código de la plataforma que ya existía antes de este cambio.

### Pruebas (salida real)
- `pnpm --filter @aule/playground typecheck` → sin errores.
- `pnpm --filter @aule/playground test` → `tests 68 · pass 68 · fail 0` (antes de esta ronda: 57).
- Cada arreglo nació con su prueba fallando (1: `signin.test.ts` y `mail.test.mjs`; 3: `mail.test.mjs`; 2 y 4: `texts.test.mjs`; rutas: `signin.test.ts`; `mail` con sesión: `signin.test.ts`).
- **No probado:** envío real de correo; Google real; móvil (solo 700 px de ancho); el Rust no se tocó ni se corrió.
