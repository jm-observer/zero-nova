import { SETTINGS_TEMPLATE_HEADER } from './settings-template/header';
import { SETTINGS_TEMPLATE_GENERAL_TAB } from './settings-template/general-tab';
import { SETTINGS_TEMPLATE_MODELS_TAB } from './settings-template/models-tab';
import { SETTINGS_TEMPLATE_TOOLS_TAB } from './settings-template/tools-tab';
import { SETTINGS_TEMPLATE_MEMORY_TAB } from './settings-template/memory-tab';

export const SETTINGS_TEMPLATE = [
    SETTINGS_TEMPLATE_HEADER,
    SETTINGS_TEMPLATE_GENERAL_TAB.replace(/^\r?\n/, ''),
    SETTINGS_TEMPLATE_MODELS_TAB.replace(/^\r?\n/, ''),
    SETTINGS_TEMPLATE_TOOLS_TAB.replace(/^\r?\n/, ''),
    SETTINGS_TEMPLATE_MEMORY_TAB.replace(/^\r?\n/, ''),
].join('');
