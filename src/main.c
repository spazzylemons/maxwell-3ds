#ifdef __INTELLISENSE__
#define M_PI 3.14159265358979f
#endif

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <3ds.h>
#include <citro3d.h>
#include <tex3ds.h>

#include "shader.h"
#include "maxwell.h"

#include "body.h"
#include "whiskers.h"

#define DISPLAY_TRANSFER_FLAGS \
    GX_TRANSFER_FLIP_VERT(0) | \
    GX_TRANSFER_OUT_TILED(0) | \
    GX_TRANSFER_RAW_COPY(0) | \
    GX_TRANSFER_IN_FORMAT(GX_TRANSFER_FMT_RGBA8) | \
    GX_TRANSFER_OUT_FORMAT(GX_TRANSFER_FMT_RGB8) | \
    GX_TRANSFER_SCALING(GX_TRANSFER_SCALE_NO)

#define CLEAR_COLOR 0x808080ff

static void *linear_copy(const void *ptr, size_t size) {
    void *result = linearAlloc(size);
    memcpy(result, ptr, size);
    return result;
}

typedef struct {
    void *vao;
    C3D_Tex tex;
    size_t len;
} Material;

static void material_init(Material *mat, const short *ptr, size_t len, const void *t3x_ptr, size_t t3x_len) {
    mat->vao = linear_copy(ptr, sizeof(short) * len);

    Tex3DS_Texture t3x = Tex3DS_TextureImport(t3x_ptr, t3x_len, &mat->tex, NULL, false);
    Tex3DS_TextureFree(t3x);

    C3D_TexSetFilter(&mat->tex, GPU_LINEAR, GPU_LINEAR);

    mat->len = len;
}

static void material_draw(Material *mat) {
    C3D_TexBind(0, &mat->tex);
    C3D_DrawElements(GPU_TRIANGLES, mat->len, C3D_UNSIGNED_SHORT, mat->vao);
}

static void material_free(Material *mat) {
    linearFree(mat->vao);
    C3D_TexDelete(&mat->tex);
}

static C3D_RenderTarget *create_target(gfx3dSide_t side) {
    C3D_RenderTarget *result = C3D_RenderTargetCreate(240, 400, GPU_RB_RGBA8, GPU_RB_DEPTH24_STENCIL8);
    C3D_RenderTargetSetOutput(result, GFX_TOP, side, DISPLAY_TRANSFER_FLAGS);
    return result;
}

static void select_target(C3D_RenderTarget *target) {
    C3D_RenderTargetClear(target, C3D_CLEAR_ALL, CLEAR_COLOR, 0);
    C3D_FrameDrawOn(target);
}

static Material body_mat, whiskers_mat;
static int shader_projection, shader_model_view;
static float angle = 4.0f;

static void render_scene(float iod) {
    C3D_Mtx projection, model_view;

    Mtx_PerspStereoTilt(&projection, C3D_AngleFromDegrees(45.0f), C3D_AspectRatioTop, 0.01f, 100.0f, iod, 3.0f, false);

    Mtx_Identity(&model_view);
    Mtx_Translate(&model_view, 0.0f, -10.0, -40.0, true);
    Mtx_RotateY(&model_view, angle, true);

    C3D_FVUnifMtx4x4(GPU_VERTEX_SHADER, shader_projection, &projection);
    C3D_FVUnifMtx4x4(GPU_VERTEX_SHADER, shader_model_view, &model_view);

    material_draw(&body_mat);
    material_draw(&whiskers_mat);
}

int main() {
    gfxInitDefault();
    gfxSet3D(true);
    C3D_Init(C3D_DEFAULT_CMDBUF_SIZE);

    C3D_RenderTarget *left = create_target(GFX_LEFT);
    C3D_RenderTarget *right = create_target(GFX_RIGHT);

    DVLB_s *shader = DVLB_ParseFile((u32 *) shader_shbin, shader_shbin_size);
    shaderProgram_s program;
    shaderProgramInit(&program);
    shaderProgramSetVsh(&program, &shader->DVLE[0]);
    C3D_BindProgram(&program);

    shader_projection = shaderInstanceGetUniformLocation(program.vertexShader, "projection");
    shader_model_view = shaderInstanceGetUniformLocation(program.vertexShader, "model_view");

    C3D_AttrInfo *attr_info = C3D_GetAttrInfo();
    AttrInfo_Init(attr_info);
    AttrInfo_AddLoader(attr_info, 0, GPU_FLOAT, 3);
    AttrInfo_AddLoader(attr_info, 1, GPU_FLOAT, 2);

    void *vertices = linear_copy(maxwell_vertices, sizeof(float) * maxwell_vertices_len * 5);

    material_init(
        &body_mat,
        maxwell_body_indices,
        maxwell_body_indices_len,
        body_t3x,
        body_t3x_size
    );

    material_init(
        &whiskers_mat,
        maxwell_whiskers_indices,
        maxwell_whiskers_indices_len,
        whiskers_t3x,
        whiskers_t3x_size
    );

    C3D_BufInfo* buf_info = C3D_GetBufInfo();
    BufInfo_Init(buf_info);
    BufInfo_Add(buf_info, vertices, sizeof(float) * 5, 2, 0x10);

    C3D_TexEnv* env = C3D_GetTexEnv(0);
    C3D_TexEnvInit(env);
    C3D_TexEnvSrc(env, C3D_Both, GPU_TEXTURE0, GPU_PRIMARY_COLOR, 0);
    C3D_TexEnvFunc(env, C3D_Both, GPU_MODULATE);

    C3D_CullFace(GPU_CULL_NONE);

    // Main loop
    while (aptMainLoop()) {
        hidScanInput();

        u32 kDown = hidKeysHeld();
        if (kDown & KEY_START) {
            break;
        }

        if (kDown & KEY_DLEFT) {
            angle -= 0.1f;
        }

        if (kDown & KEY_DRIGHT) {
            angle += 0.1f;
        }

        angle = fmodf((angle + 2.0f * M_PI), 2.0f * M_PI);

        float depth = osGet3DSliderState() * 0.125f;

        C3D_FrameBegin(C3D_FRAME_SYNCDRAW);

        select_target(left);
        render_scene(-depth);
        if (depth > 0.0f) {
            select_target(right);
            render_scene(depth);
        }
        C3D_FrameEnd(0);
    }

    linearFree(vertices);

    material_free(&body_mat);
    material_free(&whiskers_mat);

    shaderProgramFree(&program);
    DVLB_Free(shader);

    C3D_Fini();
    gfxExit();
    return 0;
}
