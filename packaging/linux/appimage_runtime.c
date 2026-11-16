// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
// Launcher runtime Type 2 compatible para Tlaxtrelom AppImage portable.

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/stat.h>
#include <signal.h>

#define TRAILER_MAGIC 0x544C5841U /* "TLXA" */

static char s_mount_dir[256] = {0};

static void cleanup_mount(void) {
    if (s_mount_dir[0]) {
        char cmd[512];
        snprintf(cmd, sizeof(cmd), "rm -rf \"%s\"", s_mount_dir);
        if (system(cmd) != 0) {
            /* ignore cleanup return */
        }
        s_mount_dir[0] = 0;
    }
}

static void sig_handler(int sig) {
    cleanup_mount();
    _exit(128 + sig);
}

int main(int argc, char** argv) {
    char exe_path[1024];
    ssize_t len = readlink("/proc/self/exe", exe_path, sizeof(exe_path) - 1);
    if (len <= 0) {
        perror("readlink /proc/self/exe");
        return 1;
    }
    exe_path[len] = 0;

    FILE* fexe = fopen(exe_path, "rb");
    if (!fexe) {
        perror("fopen self");
        return 1;
    }

    // Read trailer at end of file: uint64_t offset + uint64_t size + uint32_t magic (20 bytes)
    if (fseek(fexe, -20, SEEK_END) != 0) {
        fprintf(stderr, "AppImage: archivo corrupto o incompleto\n");
        fclose(fexe);
        return 1;
    }

    uint64_t payload_offset = 0;
    uint64_t payload_size = 0;
    uint32_t magic = 0;
    if (fread(&payload_offset, sizeof(uint64_t), 1, fexe) != 1 ||
        fread(&payload_size, sizeof(uint64_t), 1, fexe) != 1 ||
        fread(&magic, sizeof(uint32_t), 1, fexe) != 1) {
        fprintf(stderr, "AppImage: error leyendo trailer de metadatos\n");
        fclose(fexe);
        return 1;
    }

    if (magic != TRAILER_MAGIC) {
        fprintf(stderr, "AppImage: formato de trailer no valido (0x%08X)\n", magic);
        fclose(fexe);
        return 1;
    }

    // Check AppImage special flags
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--appimage-help") == 0) {
            printf("Tlaxtrelom Engine AppImage Portable\n");
            printf("Uso: %s [OPCIONES]\n", argv[0]);
            printf("  --appimage-extract    Extrae el contenido de la aplicacion a ./squashfs-root/\n");
            printf("  --appimage-help       Muestra esta ayuda\n");
            printf("  --appimage-version    Muestra la version del empaquetado\n");
            fclose(fexe);
            return 0;
        }
        if (strcmp(argv[i], "--appimage-version") == 0) {
            printf("Tlaxtrelom Engine 0.4.0-alpha.1 (AppImage Portable)\n");
            fclose(fexe);
            return 0;
        }
        if (strcmp(argv[i], "--appimage-extract") == 0) {
            printf("Extrayendo AppDir a ./squashfs-root...\n");
            mkdir("squashfs-root", 0755);
            fseek(fexe, (long)payload_offset, SEEK_SET);
            FILE* ptar = popen("tar -xz -C squashfs-root --strip-components=1", "w");
            if (!ptar) { perror("popen tar"); fclose(fexe); return 1; }
            char buf[8192];
            uint64_t remaining = payload_size;
            while (remaining > 0) {
                size_t to_read = (remaining > sizeof(buf)) ? sizeof(buf) : (size_t)remaining;
                size_t n = fread(buf, 1, to_read, fexe);
                if (n == 0) break;
                fwrite(buf, 1, n, ptar);
                remaining -= n;
            }
            pclose(ptar);
            fclose(fexe);
            printf("Extraccion completada con exito en ./squashfs-root\n");
            return 0;
        }
    }

    // Create temp mount dir
    snprintf(s_mount_dir, sizeof(s_mount_dir), "/tmp/.mount_tlaxtrelom_%d_XXXXXX", getpid());
    if (!mkdtemp(s_mount_dir)) {
        perror("mkdtemp");
        fclose(fexe);
        return 1;
    }

    atexit(cleanup_mount);
    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);
    signal(SIGHUP, sig_handler);

    // Extract payload to mount dir
    fseek(fexe, (long)payload_offset, SEEK_SET);
    char cmd[512];
    snprintf(cmd, sizeof(cmd), "tar -xz -C \"%s\" --strip-components=1", s_mount_dir);
    FILE* ptar = popen(cmd, "w");
    if (!ptar) {
        perror("popen tar");
        cleanup_mount();
        fclose(fexe);
        return 1;
    }
    char buf[8192];
    uint64_t remaining = payload_size;
    while (remaining > 0) {
        size_t to_read = (remaining > sizeof(buf)) ? sizeof(buf) : (size_t)remaining;
        size_t n = fread(buf, 1, to_read, fexe);
        if (n == 0) break;
        fwrite(buf, 1, n, ptar);
        remaining -= n;
    }
    pclose(ptar);
    fclose(fexe);

    // Prepare child exec
    char apprun[512];
    snprintf(apprun, sizeof(apprun), "%s/AppRun", s_mount_dir);

    // Set environment variables
    setenv("APPDIR", s_mount_dir, 1);
    char owd[1024];
    if (getcwd(owd, sizeof(owd))) {
        setenv("OWD", owd, 1);
    }

    pid_t pid = fork();
    if (pid == 0) {
        char** child_argv = malloc((argc + 1) * sizeof(char*));
        child_argv[0] = apprun;
        for (int i = 1; i < argc; i++) {
            child_argv[i] = argv[i];
        }
        child_argv[argc] = NULL;
        execv(apprun, child_argv);
        perror("execv AppRun");
        _exit(127);
    }

    int status = 0;
    waitpid(pid, &status, 0);

    cleanup_mount();

    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }
    if (WIFSIGNALED(status)) {
        return 128 + WTERMSIG(status);
    }
    return 1;
}
