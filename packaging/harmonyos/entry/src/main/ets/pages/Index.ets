// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
// Pagina unica: un XComponent que ocupa la ventana y una barra de estado.
// El id de superficie se reserva para el envoltorio NAPI que llamara a
// tlaxtrelom_harmonyos_init_surface / on_window_resize / destroy_surface
// desde el hilo de UI.
@Entry
@Component
struct Index {
  @StorageLink('dimensionMode') dimension: string = '3.5d';
  @StorageLink('enginePaused') paused: boolean = false;
  @State surfaceId: string = '';

  build() {
    Column() {
      XComponent({
        id: 'tlaxtrelomSurface',
        type: XComponentType.SURFACE
      })
        .onLoad((context) => {
          this.surfaceId = context?.getXComponentSurfaceId() ?? '';
        })
        .onDestroy(() => {
          this.surfaceId = '';
        })
        .backgroundColor('#12101c')
        .width('100%')
        .height('100%')

      Row() {
        Text(`Tlaxtrelom · ${this.dimension}`)
          .fontSize(14)
          .fontColor('#7ee787')
        Blank()
        Text(this.paused ? 'en segundo plano' : 'renderizando')
          .fontSize(14)
          .fontColor('#8b949e')
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 8, bottom: 8 })
      .backgroundColor('#161b22')
    }
    .width('100%')
    .height('100%')
  }
}
