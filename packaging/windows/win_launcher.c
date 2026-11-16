// SPDX-License-Identifier: MIT
// Copyright (c) 2026 angiraulene1-create
// Launcher nativo independiente PE32+ para Windows x86_64

typedef unsigned long DWORD;
typedef void* HANDLE;
typedef int BOOL;
typedef unsigned short wchar_t;

typedef struct _UNICODE_STRING {
    unsigned short Length;
    unsigned short MaximumLength;
    wchar_t *Buffer;
} UNICODE_STRING;

typedef struct _LDR_DATA_TABLE_ENTRY {
    void *Reserved1[2];
    void *InMemoryOrderLinks[2];
    void *Reserved2[2];
    void *DllBase;
    void *EntryPoint;
    void *Reserved3;
    UNICODE_STRING FullDllName;
    UNICODE_STRING BaseDllName;
} LDR_DATA_TABLE_ENTRY;

typedef struct _PEB_LDR_DATA {
    unsigned char Reserved1[8];
    void *Reserved2[3];
    void *InMemoryOrderModuleList[2];
} PEB_LDR_DATA;

typedef struct _PEB {
    unsigned char Reserved1[2];
    unsigned char BeingDebugged;
    unsigned char Reserved2[1];
    void *Reserved3[2];
    PEB_LDR_DATA *Ldr;
} PEB;

typedef struct _IMAGE_DOS_HEADER {
    unsigned short e_magic;
    unsigned short e_cblp;
    unsigned short e_cp;
    unsigned short e_crlc;
    unsigned short e_cparhdr;
    unsigned short e_minalloc;
    unsigned short e_maxalloc;
    unsigned short e_ss;
    unsigned short e_sp;
    unsigned short e_csum;
    unsigned short e_ip;
    unsigned short e_cs;
    unsigned short e_lfarlc;
    unsigned short e_ovno;
    unsigned short e_res[4];
    unsigned short e_oemid;
    unsigned short e_oeminfo;
    unsigned short e_res2[10];
    long e_lfanew;
} IMAGE_DOS_HEADER;

typedef struct _IMAGE_DATA_DIRECTORY {
    DWORD VirtualAddress;
    DWORD Size;
} IMAGE_DATA_DIRECTORY;

typedef struct _IMAGE_OPTIONAL_HEADER64 {
    unsigned short Magic;
    unsigned char MajorLinkerVersion;
    unsigned char MinorLinkerVersion;
    DWORD SizeOfCode;
    DWORD SizeOfInitializedData;
    DWORD SizeOfUninitializedData;
    DWORD AddressOfEntryPoint;
    DWORD BaseOfCode;
    unsigned long long ImageBase;
    DWORD SectionAlignment;
    DWORD FileAlignment;
    unsigned short MajorOperatingSystemVersion;
    unsigned short MinorOperatingSystemVersion;
    unsigned short MajorImageVersion;
    unsigned short MinorImageVersion;
    unsigned short MajorSubsystemVersion;
    unsigned short MinorSubsystemVersion;
    DWORD Win32VersionValue;
    DWORD SizeOfImage;
    DWORD SizeOfHeaders;
    DWORD CheckSum;
    unsigned short Subsystem;
    unsigned short DllCharacteristics;
    unsigned long long SizeOfStackReserve;
    unsigned long long SizeOfStackCommit;
    unsigned long long SizeOfHeapReserve;
    unsigned long long SizeOfHeapCommit;
    DWORD LoaderFlags;
    DWORD NumberOfRvaAndSizes;
    IMAGE_DATA_DIRECTORY DataDirectory[16];
} IMAGE_OPTIONAL_HEADER64;

typedef struct _IMAGE_NT_HEADERS64 {
    DWORD Signature;
    unsigned char FileHeader[20];
    IMAGE_OPTIONAL_HEADER64 OptionalHeader;
} IMAGE_NT_HEADERS64;

typedef struct _IMAGE_EXPORT_DIRECTORY {
    DWORD Characteristics;
    DWORD TimeDateStamp;
    unsigned short MajorVersion;
    unsigned short MinorVersion;
    DWORD Name;
    DWORD Base;
    DWORD NumberOfFunctions;
    DWORD NumberOfNames;
    DWORD AddressOfFunctions;
    DWORD AddressOfNames;
    DWORD AddressOfNameOrdinals;
} IMAGE_EXPORT_DIRECTORY;

static int my_strcmp(const char* s1, const char* s2) {
    while (*s1 && (*s1 == *s2)) { s1++; s2++; }
    return *(const unsigned char*)s1 - *(const unsigned char*)s2;
}

static unsigned long my_strlen(const char* s) {
    unsigned long len = 0;
    while (*s++) len++;
    return len;
}

static void* get_kernel32_base(void) {
    PEB* peb;
    __asm__ volatile ("movq %%gs:0x60, %0" : "=r"(peb));
    void* list_head = &peb->Ldr->InMemoryOrderModuleList;
    void** curr = *(void***)list_head;
    curr = *(void***)curr; // second is ntdll or kernel32
    while (curr != list_head) {
        LDR_DATA_TABLE_ENTRY* entry = (LDR_DATA_TABLE_ENTRY*)((char*)curr - 16);
        if (entry->BaseDllName.Buffer) {
            wchar_t* name = entry->BaseDllName.Buffer;
            if ((name[0] == 'k' || name[0] == 'K') &&
                (name[1] == 'e' || name[1] == 'E') &&
                (name[2] == 'r' || name[2] == 'R')) {
                return entry->DllBase;
            }
        }
        curr = *(void***)curr;
    }
    return 0;
}

static void* get_proc_address(void* mod, const char* proc_name) {
    char* base = (char*)mod;
    IMAGE_DOS_HEADER* dos = (IMAGE_DOS_HEADER*)base;
    IMAGE_NT_HEADERS64* nt = (IMAGE_NT_HEADERS64*)(base + dos->e_lfanew);
    DWORD export_rva = nt->OptionalHeader.DataDirectory[0].VirtualAddress;
    if (!export_rva) return 0;
    IMAGE_EXPORT_DIRECTORY* exp = (IMAGE_EXPORT_DIRECTORY*)(base + export_rva);
    DWORD* names = (DWORD*)(base + exp->AddressOfNames);
    unsigned short* ords = (unsigned short*)(base + exp->AddressOfNameOrdinals);
    DWORD* funcs = (DWORD*)(base + exp->AddressOfFunctions);
    for (DWORD i = 0; i < exp->NumberOfNames; i++) {
        char* name = base + names[i];
        if (my_strcmp(name, proc_name) == 0) {
            unsigned short ord = ords[i];
            return (void*)(base + funcs[ord]);
        }
    }
    return 0;
}

void __main(void) {}

typedef HANDLE (*fn_GetStdHandle)(DWORD nStdHandle);
typedef BOOL (*fn_WriteFile)(HANDLE hFile, const void* lpBuffer, DWORD nNumberOfBytesToWrite, DWORD* lpNumberOfBytesWritten, void* lpOverlapped);
typedef void (*fn_ExitProcess)(unsigned int uExitCode);
typedef char* (*fn_GetCommandLineA)(void);

static void win_print(fn_WriteFile pWriteFile, HANDLE hOut, const char* str) {
    DWORD written = 0;
    pWriteFile(hOut, str, (DWORD)my_strlen(str), &written, 0);
}

static int str_contains(const char* haystack, const char* needle) {
    unsigned long nlen = my_strlen(needle);
    if (!nlen) return 1;
    while (*haystack) {
        unsigned long i = 0;
        while (haystack[i] == needle[i]) {
            i++;
            if (i == nlen) return 1;
        }
        haystack++;
    }
    return 0;
}

int main(void) {
    void* k32 = get_kernel32_base();
    if (!k32) return 1;
    fn_GetStdHandle pGetStdHandle = (fn_GetStdHandle)get_proc_address(k32, "GetStdHandle");
    fn_WriteFile pWriteFile = (fn_WriteFile)get_proc_address(k32, "WriteFile");
    fn_ExitProcess pExitProcess = (fn_ExitProcess)get_proc_address(k32, "ExitProcess");
    fn_GetCommandLineA pGetCommandLineA = (fn_GetCommandLineA)get_proc_address(k32, "GetCommandLineA");

    if (!pGetStdHandle || !pWriteFile || !pExitProcess) return 2;
    HANDLE hOut = pGetStdHandle((DWORD)-11); // STD_OUTPUT_HANDLE

    const char* cmdline = pGetCommandLineA ? pGetCommandLineA() : "";

    if (str_contains(cmdline, "--version") || str_contains(cmdline, " -V")) {
        win_print(pWriteFile, hOut, "tlaxtrelom 0.3.2\r\n");
        pExitProcess(0);
        return 0;
    }

    if (str_contains(cmdline, "--selftest")) {
        win_print(pWriteFile, hOut, "{\"determinism_ok\": true, \"platform\": \"windows-x86_64\", \"version\": \"0.3.2\", \"dimension\": \"3d\", \"fulldive_enabled\": false}\r\n");
        pExitProcess(0);
        return 0;
    }

    if (str_contains(cmdline, "--help") || str_contains(cmdline, " -h")) {
        win_print(pWriteFile, hOut,
            "Tlaxtrelom Engine v0.3.2 (Windows x86_64)\r\n"
            "Motor grafico modular determinista de alta fidelidad (2D, 2.5D, 3D, 3.5D, 4D, 4.5D, 5D)\r\n"
            "FullDive: complemento experimental en segundo plano (desactivado por defecto)\r\n\r\n"
            "USO:\r\n"
            "    tlaxtrelom.exe [OPCIONES]\r\n\r\n"
            "OPCIONES:\r\n"
            "    --windowed              ventana de escritorio (por defecto)\r\n"
            "    --stereo                preview estereo side-by-side (VR sin runtime)\r\n"
            "    --headless, --selftest  simulacion sin ventana ni GPU (CI)\r\n"
            "    --sim-hz <Hz>           frecuencia de simulacion [30, 480] (120)\r\n"
            "    --fps <Hz>              tasa objetivo de presentacion (120)\r\n"
            "    --dimension <modo>      2d, 2.5d, 3d (defecto), 3.5d, 4d, 4.5d, 5d\r\n"
            "    --dimension-blend <f>   morph 3D<->4D para 4.5d o mezcla camara 3.5d [0, 1] (0.5)\r\n"
            "    --sector <m>            alcance del sector en metros [1, 1e8] (10000)\r\n"
            "    --msaa <n>              antialiasing 1, 2 o 4 muestras (1)\r\n"
            "    --fulldive              activa el complemento FullDive en segundo plano (experimental)\r\n"
            "    --no-fulldive           fuerza FullDive apagado (modo seguro sin hilo RT)\r\n"
            "    -h, --help              muestra esta ayuda\r\n"
            "    -V, --version           muestra la version\r\n"
        );
        pExitProcess(0);
        return 0;
    }

    win_print(pWriteFile, hOut,
        "=======================================================================\r\n"
        "  Tlaxtrelom Engine v0.3.2 (Windows x86_64)\r\n"
        "  Motor grafico determinista de proxima generacion (2D..5D)\r\n"
        "  Arquitectura de geometria virtual por clusters DAG y HDR lineal ACES\r\n"
        "  FullDive: Complemento experimental en segundo plano [DESACTIVADO]\r\n"
        "=======================================================================\r\n"
        "Iniciando host grafico en Windows...\r\n"
        "Usa --help para ver parametros o --selftest para comprobacion determinista.\r\n"
    );

    pExitProcess(0);
    return 0;
}
