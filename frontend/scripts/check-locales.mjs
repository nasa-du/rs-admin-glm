// Locale key alignment gate (issue #20, ADR 0002 / ADR 0004): the zh-CN and
// en locale files must carry the exact same key set, so bilingual copy stays
// in lockstep. Zero-dependency on purpose — it runs in CI (frontend
// workflow) with no toolchain beyond Node itself.
//
// Exit codes: 0 when the key sets match, 1 on any divergence (or on a
// missing / unparsable locale file), with every divergent key listed so the
// failing line in CI is actionable.

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const frontendRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const LOCALES = ['zh-CN', 'en']

// Flatten a parsed locale file into i18next dot paths (default
// `keySeparator: '.'`). Plain nested objects recurse; every other leaf
// (string, array, number) counts as a key of its own.
function flatten(value, prefix = '', out = []) {
  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (child !== null && typeof child === 'object' && !Array.isArray(child)) {
      flatten(child, path, out)
    } else {
      out.push(path)
    }
  }
  return out
}

function loadKeys(locale) {
  const file = join(frontendRoot, 'src', 'locales', `${locale}.json`)
  try {
    return new Set(flatten(JSON.parse(readFileSync(file, 'utf8'))))
  } catch (error) {
    console.error(`locale-check: cannot read or parse ${file}`)
    console.error(`  ${error.message}`)
    process.exit(1)
  }
}

const keysByLocale = new Map(LOCALES.map((locale) => [locale, loadKeys(locale)]))

let diverged = false
for (const [locale, other] of [
  ['zh-CN', 'en'],
  ['en', 'zh-CN'],
]) {
  const gaps = [...keysByLocale.get(locale)]
    .filter((key) => !keysByLocale.get(other).has(key))
    .sort()
  if (gaps.length > 0) {
    diverged = true
    console.error(
      `locale-check: ${gaps.length} key(s) present in ${locale}.json but missing from ${other}.json:`
    )
    for (const key of gaps) {
      console.error(`  ${key}`)
    }
  }
}

if (diverged) {
  console.error(
    'locale-check: key sets diverge — add the missing keys to both files in the same commit (ADR 0002).'
  )
  process.exit(1)
}

console.log(
  `locale-check: zh-CN and en key sets aligned (${keysByLocale.get('zh-CN').size} keys)`
)
