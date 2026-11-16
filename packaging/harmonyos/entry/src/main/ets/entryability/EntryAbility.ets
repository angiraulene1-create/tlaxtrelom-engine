// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
// Ability de entrada de Tlaxtrelom en HarmonyOS.
// Levanta la ventana y entrega el XComponent al hilo de render. El puente
// nativo (libtlaxtrelom_harmonyos.so) se expone al ArkTS a traves del envoltorio
// NAPI `libtlaxtrelom_napi.so`, que aun no existe: ver packaging/harmonyos/README.
import { AbilityConstant, UIAbility, Want } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { hilog } from '@kit.PerformanceAnalysisKit';

const DOMAIN: number = 0x1a2b;
const TAG: string = 'Tlaxtrelom';

export default class EntryAbility extends UIAbility {
  onCreate(want: Want, launchParam: AbilityConstant.LaunchParam): void {
    const mode: string = want.parameters?.['dimension'] as string ?? '3.5d';
    AppStorage.setOrCreate('dimensionMode', mode);
    hilog.info(DOMAIN, TAG, 'abilidad creada, modo %{public}s', mode);
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(DOMAIN, TAG, 'fallo al cargar la pagina: %{public}d', err.code);
        return;
      }
      hilog.info(DOMAIN, TAG, 'pagina cargada');
    });
  }

  onForeground(): void {
    AppStorage.setOrCreate('enginePaused', false);
  }

  onBackground(): void {
    AppStorage.setOrCreate('enginePaused', true);
  }
}
