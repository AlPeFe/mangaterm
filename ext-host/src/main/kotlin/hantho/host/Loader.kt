package hantho.host

import eu.kanade.tachiyomi.source.CatalogueSource
import eu.kanade.tachiyomi.source.Source
import eu.kanade.tachiyomi.source.SourceFactory
import java.io.File
import java.net.URLClassLoader
import java.util.concurrent.ConcurrentHashMap
import java.util.jar.JarFile

class LoadedExtension(val pkg: String, val jar: File, val sources: List<Source>, val loader: URLClassLoader, val copy: File)

object Loader {
    val sources = ConcurrentHashMap<Long, Source>()
    private val extensions = ConcurrentHashMap<String, LoadedExtension>()

    private val metaClass = Regex("""android:name="tachiyomi\.extension\.class"\s+android:value="([^"]+)"""")
    private val manifestPackage = Regex("""package="([^"]+)"""")

    /**
     * Copias privadas de los jar cargados. La JVM lee clases del jar a medida que las necesita, así
     * que el original no puede cambiar mientras está cargado: con la copia, instalar o actualizar una
     * extensión no rompe a ningún host que ya la tenga cargada (la TUI, el MCP de un agente…).
     */
    private val copies = File(HostDirs.cache, "jars").apply {
        mkdirs()
        // Restos de otras sesiones (los que sigan en uso por otro host no se dejan borrar)
        val old = System.currentTimeMillis() - 24 * 60 * 60 * 1000L
        listFiles()?.filter { it.lastModified() < old }?.forEach { it.delete() }
    }

    fun load(jarPath: String): LoadedExtension {
        val original = File(jarPath).absoluteFile
        val jar = File(copies, "${original.nameWithoutExtension}-${System.nanoTime()}.jar")
        original.copyTo(jar)
        val manifest = JarFile(jar).use { jf ->
            val entry = jf.getJarEntry("AndroidManifest.xml") ?: error("El jar no tiene AndroidManifest.xml")
            jf.getInputStream(entry).bufferedReader().readText()
        }
        val pkg = manifestPackage.find(manifest)?.groupValues?.get(1) ?: jar.nameWithoutExtension
        val classNames = metaClass.find(manifest)?.groupValues?.get(1)
            ?: error("Falta meta-data tachiyomi.extension.class")

        // Un classloader por extensión: cada jar trae su propia copia (ofuscada) de keiyoushi core
        val loader = URLClassLoader(arrayOf(jar.toURI().toURL()), Loader::class.java.classLoader)
        val created = classNames.split(';').map { it.trim() }.filter { it.isNotEmpty() }.flatMap { name ->
            val fqcn = if (name.startsWith(".")) pkg + name else name
            when (val obj = Class.forName(fqcn, false, loader).getDeclaredConstructor().newInstance()) {
                is Source -> listOf(obj)
                is SourceFactory -> obj.createSources()
                else -> error("$fqcn no es Source ni SourceFactory")
            }
        }
        // Recarga (p. ej. tras actualizar): fuera las fuentes y el classloader de la versión anterior
        extensions[pkg]?.let { previous ->
            previous.sources.forEach { sources.remove(it.id) }
            runCatching { previous.loader.close() }
            previous.copy.delete()
        }
        created.forEach { sources[it.id] = it }
        return LoadedExtension(pkg, original, created, loader, jar).also { extensions[pkg] = it }
    }

    fun source(id: Long): CatalogueSource =
        sources[id] as? CatalogueSource ?: error("Fuente $id no cargada")
}
