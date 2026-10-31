plugins {
    kotlin("jvm") version "2.4.10"
    kotlin("plugin.serialization") version "2.4.10"
    application
}

// Mismas versiones que keiyoushi/extensions-source (gradle/libs.versions.toml),
// porque las extensiones se compilan contra ellas.
val okhttp = "5.4.0"
val serialization = "1.11.0"

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:$serialization")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json-okio:$serialization")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-protobuf:$serialization")
    implementation("com.squareup.okhttp3:okhttp:$okhttp")
    implementation("com.squareup.okhttp3:okhttp-brotli:$okhttp")
    implementation("com.squareup.okhttp3:okhttp-zstd:$okhttp")
    implementation("com.squareup.zstd:zstd-kmp-okio:0.4.0")
    implementation("org.jsoup:jsoup:1.22.2")
    // WebP para BitmapFactory (ImageIO no lo trae): muchas webs sirven así las páginas troceadas
    implementation("com.twelvemonkeys.imageio:imageio-webp:3.12.0")
    implementation("com.github.null2264.injekt:injekt-core:4135455a2a")
}

kotlin {
    jvmToolchain(21)
}

application {
    mainClass.set("hantho.host.MainKt")
}
