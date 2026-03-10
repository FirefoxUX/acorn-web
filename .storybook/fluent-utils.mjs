/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

import { DOMLocalization } from "@fluent/dom";
import { FluentBundle, FluentResource } from "@fluent/bundle";
import { addons } from "storybook/preview-api";
import { PSEUDO_STRATEGY_TRANSFORMS } from "./l10n-pseudo.mjs";
import {
  FLUENT_SET_STRINGS,
  UPDATE_STRATEGY_EVENT,
  STRATEGY_DEFAULT,
  PSEUDO_STRATEGIES,
} from "./addon-fluent/constants.mjs";

// Vite: Eagerly import all extracted .ftl files as raw text
const ftlModules = import.meta.glob(
  '../dist/locales/en-US/**/*.ftl',
  { eager: true, query: '?raw', import: 'default' }
);

let loadedResources = new Map();
let currentStrategy;
let storybookBundle = new FluentBundle("en-US", {
  transform(str) {
    if (currentStrategy in PSEUDO_STRATEGY_TRANSFORMS) {
      return PSEUDO_STRATEGY_TRANSFORMS[currentStrategy](str);
    }
    return str;
  },
});

// Listen for update events from addon-fluent.
const channel = addons.getChannel();
channel.on(UPDATE_STRATEGY_EVENT, updatePseudoStrategy);
channel.on(FLUENT_SET_STRINGS, ftlContents => {
  let resource = new FluentResource(ftlContents);
  for (let message of resource.body) {
    let existingMessage = storybookBundle.getMessage(message.id);
    existingMessage.value = message.value;
    existingMessage.attributes = message.attributes;
  }
  document.l10n.translateRoots();
});

/**
 * Updates "currentStrategy" when the selected pseudo localization strategy
 * changes, which in turn changes the transform used by the Fluent bundle.
 *
 * @param {string} strategy
 *  Pseudo localization strategy. Can be "default", "accented", or "bidi".
 */
function updatePseudoStrategy(strategy = STRATEGY_DEFAULT) {
  if (strategy !== currentStrategy && PSEUDO_STRATEGIES.includes(strategy)) {
    currentStrategy = strategy;
    document.l10n.translateRoots();
  }
}

export function connectFluent() {
  document.l10n = new DOMLocalization([], generateBundles);
  document.l10n.connectRoot(document.documentElement);
  document.l10n.translateRoots();
}

function* generateBundles() {
  yield* [storybookBundle];
}

export async function insertFTLIfNeeded(fileName) {
  if (loadedResources.has(fileName)) {
    return;
  }

  // Look up in extracted FTL modules (from dist/locales/en-US/)
  const path = `../dist/locales/en-US/${fileName}`;
  const ftlContents = ftlModules[path];

  if (!ftlContents) {
    console.warn(`FTL file not found for: ${fileName}`);
    return;
  }

  provideFluent(ftlContents, fileName);
}

export function provideFluent(ftlContents, fileName) {
  console.debug(`Providing Fluent resource: ${fileName}`);
  let ftlResource = new FluentResource(ftlContents);
  storybookBundle.addResource(ftlResource);
  if (fileName) {
    loadedResources.set(fileName, ftlResource);
  }
  document.l10n.translateRoots();
  return ftlResource;
}
