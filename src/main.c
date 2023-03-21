#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <3ds.h>

int main() {
    gfxInitDefault();
    consoleInit(GFX_TOP, NULL);

    printf("hello world\n");

    while (aptMainLoop()) {
        gspWaitForVBlank();
        gfxSwapBuffers();
        hidScanInput();
        if (hidKeysDown() & KEY_START) break;
    }

    gfxExit();

    return 0;
}
